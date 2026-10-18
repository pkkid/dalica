//! The face scan (`roadmap.md` M13.18; `faces.md` §4–§5): a synthetic detector and recogniser
//! planted in a models folder find the painted faces of a `scenes` set, which appear under Unnamed
//! in groups; removed and ignored faces stay away on a rescan and Find Faces Again finds them; a
//! folder that does not find faces is never read; the scan yields while the user is busy and the
//! owner thread makes no `fs` call; switching models rebuilds from each model's own cache; turning
//! the scan off can drop the cached data; a `trace` log holds no name, rectangle or embedding.
//!
//! The detector is a network of the real one's shape whose outputs are planted constants (two
//! faces, left and right, on every photo); the recogniser averages the face's colours, so faces
//! painted in one colour embed alike and the groups follow the paint.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use dalica_core::{PhotoId, SystemClock};
use dalica_engine::Session;
use dalica_engine::faces_scan::UserActivity;
use dalica_faces::graph::{Attr, Graph, Node, Weight};
use dalica_faces::known;
use dalica_faces::manifest::{InputSpec, ModelManifest, OutputSpec};
use dalica_faces::synthetic::encode;
use dalica_fs::Expect;
use serde_json::{Value, json};

use crate::common::{open, open_with, virtual_clock};

const WAIT: Duration = Duration::from_secs(120);

/// The photos' size; the detector's input side (the photo fills its top 64 × 48).
const W: u32 = 256;
const H: u32 = 192;
const SIDE: usize = 64;

const RED: [u8; 3] = [220, 40, 40];
const BLUE: [u8; 3] = [40, 40, 220];
const GREEN: [u8; 3] = [40, 220, 40];

/// The detector: the twelve outputs of the real one's shape, as planted constants: two faces of
/// score 0.9 in the stride-8 grid at row 2, columns 1 and 5 (16 × 12 input pixels each: on the
/// photo `[0.0625, 0.2917, 0.3125, 0.5417]` and `[0.5625, …, 0.8125, …]`), with landmarks.
fn detector_bytes() -> Vec<u8> {
    const KPS: [f32; 10] = [0.25, 0.25, 0.75, 0.25, 0.5, 0.5, 0.3, 0.75, 0.7, 0.75];
    let plants = [(2usize, 1usize), (2, 5)];
    let (mut nodes, mut weights, mut outputs) = (Vec::new(), Vec::new(), Vec::new());
    for stride in [8usize, 16, 32] {
        let cols = SIDE / stride;
        let cells = cols * cols;
        for (kind, n) in [("cls", 1usize), ("obj", 1), ("bbox", 4), ("kps", 10)] {
            let mut values = vec![0.0f32; cells * n];
            if stride == 8 {
                for (row, col) in plants {
                    let at = (row * cols + col) * n;
                    let v: Vec<f32> = match kind {
                        "cls" => vec![0.81],
                        "obj" => vec![1.0],
                        "bbox" => vec![0.5, 0.5, 2f32.ln(), 1.5f32.ln()],
                        _ => KPS.to_vec(),
                    };
                    values[at..at + n].copy_from_slice(&v);
                }
            }
            let name = format!("{kind}_{stride}");
            let weight = format!("w_{name}");
            nodes.push(Node::new("Identity", &[&weight], &name, &[]));
            weights.push((weight, Weight::f32(&[1, cells, n], values)));
            outputs.push(name);
        }
    }
    encode(&Graph { nodes, weights: weights.into_iter().collect(), inputs: vec![("input".into(), vec![1, 3, SIDE, SIDE])], outputs })
}

/// A recogniser that averages the face's colours: a 1 × 1 convolution by `mix` (3 × 3), global
/// average pooling and flattening to `[1, 3]`.
fn recogniser_bytes(mix: [f32; 9]) -> Vec<u8> {
    let nodes = vec![
        Node::new("Conv", &["data", "w", "b"], "c", &[("kernel_shape", Attr::Ints(vec![1, 1]))]),
        Node::new("GlobalAveragePool", &["c"], "p", &[]),
        Node::new("Flatten", &["p"], "emb", &[("axis", Attr::Int(1))]),
    ];
    let weights = [("w".to_string(), Weight::f32(&[3, 3, 1, 1], mix.to_vec())), ("b".to_string(), Weight::f32(&[3], vec![0.0; 3]))];
    encode(&Graph { nodes, weights: weights.into_iter().collect(), inputs: vec![("data".into(), vec![1, 3, 112, 112])], outputs: vec!["emb".into()] })
}

const RECOGNISER_A: &str = "test-recogniser-a";
const RECOGNISER_B: &str = "test-recogniser-b";

/// Installs `bytes` as model `manifest.id` in `models` (its folder, `model.onnx` and
/// `face-model.json`, as an install leaves them).
fn install(models: &Path, mut manifest: ModelManifest, bytes: &[u8]) {
    manifest.sha256 = Some(dalica_faces::hash::sha256_hex(bytes));
    manifest.size_bytes = Some(bytes.len() as u64);
    let dir = models.join(&manifest.id);
    dalica_fs::create_dir_all(&dir).unwrap();
    dalica_fs::replace(&dir.join("model.onnx"), bytes, Expect::Absent).unwrap();
    dalica_fs::replace(&dir.join("face-model.json"), &serde_json::to_vec_pretty(&manifest).unwrap(), Expect::Absent).unwrap();
}

/// A models folder with the planted detector and recogniser A (and B, which mixes the colours
/// another way, when `both`).
fn models(root: &Path, both: bool) -> PathBuf {
    let models = root.join("models");
    let mut det = known::yunet();
    det.id = "test-detector".into();
    det.input.width = SIDE as u32;
    det.input.height = SIDE as u32;
    install(&models, det, &detector_bytes());
    let recogniser = |id: &str| {
        let mut m = known::sface();
        m.id = id.into();
        m.input = InputSpec::default();
        m.output = OutputSpec::Embedding { dim: 3 };
        m.thresholds.match_cosine = Some(0.5);
        m.calibration = None;
        m
    };
    install(&models, recogniser(RECOGNISER_A), &recogniser_bytes([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]));
    if both {
        install(&models, recogniser(RECOGNISER_B), &recogniser_bytes([0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0]));
    }
    models
}

/// A PNG of scene `seed` with a face painted in `left` and one in `right`, each well around the
/// box the detector reports there.
fn photo(seed: u64, left: [u8; 3], right: [u8; 3]) -> Vec<u8> {
    let img = dalica_scenes::Scene::new(seed, W, H).render().unwrap();
    let mut rgba = Vec::with_capacity((W * H * 4) as usize);
    for y in 0..H {
        for x in 0..W {
            let paint = if (44..116).contains(&y) && (0..96).contains(&x) {
                Some(left)
            } else if (44..116).contains(&y) && (128..224).contains(&x) {
                Some(right)
            } else {
                None
            };
            let [r, g, b] = paint.unwrap_or_else(|| img.get(x, y).unwrap());
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
    }
    dalica_scenes::fingerprint::png_rgba(W, H, &rgba)
}

/// Writes `photos` into `folder` as `IMG_0001.png` …
fn write_photos(folder: &Path, photos: &[Vec<u8>]) {
    dalica_fs::create_dir_all(folder).unwrap();
    for (i, bytes) in photos.iter().enumerate() {
        dalica_fs::replace(&folder.join(format!("IMG_{:04}.png", i + 1)), bytes, Expect::Absent).unwrap();
    }
}

/// Watches `folders` (scanned) in session `s`; the photos' ids by folder and name.
fn watch(s: &mut Session, folders: &[&Path]) -> BTreeMap<(PathBuf, String), PhotoId> {
    for f in folders {
        s.execute("folders.add", &json!({ "path": f })).unwrap();
    }
    s.wait_idle();
    ids(s)
}

fn ids(s: &Session) -> BTreeMap<(PathBuf, String), PhotoId> {
    let lib = s.library();
    lib.photos().map(|p| ((lib.folder(p.folder).unwrap().path.clone(), p.name.clone()), p.id)).collect()
}

fn index(s: &mut Session) -> Value {
    s.execute("faces.index", &json!({})).unwrap()
}

fn stat(s: &mut Session, key: &str) -> u64 {
    index(s)["stats"][key].as_u64().unwrap()
}

/// The photo's faces: (left or right, unnamed / ignored / named).
fn faces_of(s: &Session, id: PhotoId) -> Vec<(&'static str, &'static str)> {
    let mut v: Vec<_> = s
        .library()
        .faces(id)
        .iter()
        .map(|f| {
            let side = if f.rect.x0() < 0.5 { "left" } else { "right" };
            let who = match f.person {
                dalica_library::FacePerson::Unnamed => "unnamed",
                dalica_library::FacePerson::Ignored => "ignored",
                dalica_library::FacePerson::Person(_) => "named",
            };
            (side, who)
        })
        .collect();
    v.sort();
    v
}

/// The scan's models folder given, and the scan waited for.
fn scan(s: &mut Session, models: &Path) {
    s.set_face_models_dir(Some(models.to_path_buf()));
    assert!(s.wait_faces(WAIT), "the face scan finished: {}", index(s));
}

#[test]
fn faces_of_a_scenes_set_appear_under_unnamed_in_groups() {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let models = models(disk.path(), false);
    let folder = disk.path().join("Lisbon");
    write_photos(&folder, &[photo(1, RED, BLUE), photo(2, RED, BLUE), photo(3, RED, BLUE), photo(4, RED, GREEN)]);
    let mut s = open(lib.path());
    let photos = watch(&mut s, &[&folder]);
    assert_eq!(index(&mut s)["state"], json!("noModel"), "no models folder: nothing is scanned");
    scan(&mut s, &models);

    let status = index(&mut s);
    assert_eq!((status["state"].clone(), status["model"].clone(), status["detector"].clone()), (json!("idle"), json!(RECOGNISER_A), json!(true)));
    assert_eq!((stat(&mut s, "read"), stat(&mut s, "searched"), stat(&mut s, "found"), stat(&mut s, "embedded")), (4, 4, 8, 8));
    let unnamed = s.execute("faces.list", &json!({ "unnamed": true })).unwrap();
    assert_eq!(unnamed["total"], json!(8), "two faces on each photo, under Unnamed");
    for id in photos.values() {
        assert_eq!(faces_of(&s, *id), [("left", "unnamed"), ("right", "unnamed")]);
    }
    // The groups follow the paint, largest first: the four red faces, the three blue ones, the green one.
    let side: HashMap<String, (PhotoId, bool)> = unnamed["faces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (f["id"].as_str().unwrap().to_string(), (PhotoId::new(f["photo"].as_u64().unwrap()), f["rect"][0].as_f64().unwrap() < 0.5)))
        .collect();
    let groups = index_groups(&mut s);
    assert_eq!(groups.iter().map(Vec::len).collect::<Vec<_>>(), [4, 3, 1], "{groups:?}");
    assert!(groups[0].iter().all(|f| side[f].1), "the red faces are the left ones");
    let fourth = photos.values().copied().max().unwrap();
    assert!(groups[1].iter().all(|f| !side[f].1 && side[f].0 != fourth), "the blue ones are on the right of the first three");
    assert_eq!(side[&groups[2][0]], (fourth, false), "the green one");
    for (n, g) in groups.iter().enumerate() {
        for f in g {
            let (photo, index) = f.split_once('.').unwrap();
            let r = dalica_library::FaceRef { photo: photo.parse().unwrap(), index: index.parse().unwrap() };
            assert_eq!(s.face_group(r), Some(n));
        }
    }
    // The scan's job ended; its progress was the People group's header.
    let job = s.face_scan().job().unwrap();
    let entry = s.job(job).unwrap();
    assert_eq!((entry.state, entry.result.clone()), (dalica_engine::jobs::JobState::Done, Some(json!({ "photos": 4, "found": 8 }))));
    // The found faces reached the sidecar.
    s.flush().unwrap();
    let sidecar = crate::common::read_json(&folder.join(".dalica.json")).unwrap();
    assert_eq!(sidecar["photos"]["IMG_0001.png"]["faces"].as_array().map(Vec::len), Some(2));
}

fn index_groups(s: &mut Session) -> Vec<Vec<String>> {
    let v = s.execute("faces.index", &json!({ "groups": true })).unwrap();
    serde_json::from_value(v["groupList"].clone()).unwrap()
}

#[test]
fn a_removed_or_ignored_face_is_not_found_again_and_find_faces_again_finds_it() {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let models = models(disk.path(), false);
    let (lisbon, porto) = (disk.path().join("Lisbon"), disk.path().join("Porto"));
    write_photos(&lisbon, &[photo(1, RED, BLUE)]);
    write_photos(&porto, &[photo(2, RED, BLUE)]);
    let mut s = open(lib.path());
    let photos = watch(&mut s, &[&lisbon, &porto]);
    let (a, b) = (photos[&(lisbon.clone(), "IMG_0001.png".into())], photos[&(porto.clone(), "IMG_0001.png".into())]);
    scan(&mut s, &models);
    // Ignore Lisbon's left face; remove the faces found in Porto (the Folder Manager asks first).
    let left = s.library().faces(a).iter().position(|f| f.rect.x0() < 0.5).unwrap();
    s.execute("faces.ignore", &json!({ "faces": [format!("{a}.{left}")] })).unwrap();
    let asked = s.execute("faces.removeInFolders", &json!({ "folders": [porto] })).unwrap_err();
    assert_eq!((asked.code(), asked.to_string().contains("Remove the face rectangles found in these folders?")), ("disabled", true));
    let removed = s.execute("faces.removeInFolders", &json!({ "folders": [porto], "confirm": true })).unwrap();
    assert_eq!(removed, json!({ "photos": 1, "removed": 2 }));
    assert!(s.library().faces(b).is_empty());
    s.flush().unwrap();
    drop(s);

    // A rescan (a new session, the cache read back): nothing is read, nothing comes back.
    let mut s = open(lib.path());
    scan(&mut s, &models);
    assert_eq!((stat(&mut s, "read"), stat(&mut s, "started")), (0, 0), "every face embedded and every photo searched already");
    assert_eq!(faces_of(&s, a), [("left", "ignored"), ("right", "unnamed")]);
    assert!(s.library().faces(b).is_empty(), "the removed faces stay removed");
    assert_eq!(s.faces_index().faces(), 4, "the cache's faces are back in the index");

    // Find Faces Again: the found faces go, and the photos are searched again, first.
    let reset = s.execute("faces.reset", &json!({ "ids": [a, b] })).unwrap();
    assert_eq!(reset, json!({ "photos": 2, "removed": 2 }));
    assert!(s.wait_faces(WAIT));
    assert_eq!(faces_of(&s, a), [("left", "unnamed"), ("right", "unnamed")], "the ignored face is found again");
    assert_eq!(faces_of(&s, b), [("left", "unnamed"), ("right", "unnamed")], "the removed faces are found again");
    assert_eq!(stat(&mut s, "searched"), 2);
}

#[test]
fn a_photo_in_a_folder_that_does_not_find_faces_is_never_read() {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let models = models(disk.path(), false);
    let (lisbon, porto) = (disk.path().join("Lisbon"), disk.path().join("Porto"));
    write_photos(&lisbon, &[photo(1, RED, BLUE), photo(2, RED, BLUE)]);
    write_photos(&porto, &[photo(3, RED, BLUE), photo(4, RED, BLUE)]);
    let mut s = open(lib.path());
    let photos = watch(&mut s, &[&lisbon, &porto]);
    s.execute("folders.set", &json!({ "changes": [{ "path": porto, "faces": false }] })).unwrap();
    scan(&mut s, &models);
    assert_eq!((stat(&mut s, "started"), stat(&mut s, "read"), stat(&mut s, "failed")), (2, 2, 0), "only Lisbon's two photos were read");
    for ((folder, _), id) in &photos {
        let want = if *folder == lisbon { 2 } else { 0 };
        assert_eq!(s.library().faces(*id).len(), want);
    }
    let excluded = photos[&(porto.clone(), "IMG_0001.png".into())];
    let asked = s.execute("faces.detect", &json!({ "ids": [excluded] })).unwrap();
    assert_eq!((asked["photos"].clone(), asked["skipped"].clone()), (json!(0), json!(1)));
    assert!(s.wait_faces(WAIT));
    assert_eq!(stat(&mut s, "started"), 2, "not even when asked by name");
}

#[test]
fn the_scan_yields_while_the_user_is_busy_and_the_owner_makes_no_fs_call() {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let models = models(disk.path(), false);
    let folder = disk.path().join("Lisbon");
    write_photos(&folder, &[photo(1, RED, BLUE), photo(2, RED, BLUE), photo(3, RED, GREEN)]);
    let clock = virtual_clock();
    let mut s = open_with(lib.path(), clock.clone());
    watch(&mut s, &[&folder]);

    let before = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    s.user_activity(UserActivity::Input);
    s.set_face_models_dir(Some(models.clone()));
    let until = Instant::now() + WAIT;
    while s.faces_index().model().is_none() {
        assert!(Instant::now() < until, "the models loaded: {}", index(&mut s));
        s.user_activity(UserActivity::Input);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(!s.wait_faces(Duration::from_millis(300)), "the photos wait while the user types");
    let status = index(&mut s);
    assert_eq!((status["state"].clone(), status["pace"].clone(), status["stats"]["started"].clone()), (json!("paused"), json!("pause"), json!(0)));
    assert_eq!(status["label"], json!("Finding faces\u{2026} 0 of 3"), "the People group's header");
    let job = s.face_scan().job().unwrap();
    assert_eq!(s.job(job).unwrap().label, "Finding faces\u{2026} 0 of 3");
    // Still within a second of the last input: still nothing starts.
    clock.advance(Duration::from_millis(900));
    s.poll_face_scan();
    assert_eq!(stat(&mut s, "started"), 0);
    // Idle: the scan goes on.
    clock.advance(Duration::from_secs(5));
    s.user_activity(UserActivity::Idle);
    assert!(s.wait_faces(WAIT));
    assert_eq!((stat(&mut s, "started"), stat(&mut s, "found")), (3, 6));
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), before, "the owner thread made no fs call");
}

#[test]
fn switching_models_rebuilds_from_each_models_own_cache() {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let models = models(disk.path(), true);
    let folder = disk.path().join("Lisbon");
    write_photos(&folder, &[photo(1, RED, BLUE), photo(2, RED, BLUE), photo(3, RED, GREEN)]);
    let mut s = open(lib.path());
    watch(&mut s, &[&folder]);
    let use_model = |s: &mut Session, id: &str| {
        s.change_settings(|st| {
            let mut p = st.people();
            p.model = Some(id.to_string());
            st.set_people(&p);
        });
    };
    use_model(&mut s, RECOGNISER_A);
    scan(&mut s, &models);
    assert_eq!((index(&mut s)["model"].clone(), stat(&mut s, "read")), (json!(RECOGNISER_A), 3));
    let groups_a = index_groups(&mut s);

    use_model(&mut s, RECOGNISER_B);
    assert!(s.wait_faces(WAIT));
    assert_eq!((index(&mut s)["model"].clone(), stat(&mut s, "read")), (json!(RECOGNISER_B), 6), "B's cache was empty: every photo read");
    assert_eq!((stat(&mut s, "found"), s.faces_index().faces()), (6, 6), "B only embedded the faces A found");

    use_model(&mut s, RECOGNISER_A);
    assert!(s.wait_faces(WAIT));
    assert_eq!((index(&mut s)["model"].clone(), stat(&mut s, "read")), (json!(RECOGNISER_A), 6), "A's own cache: nothing read");
    assert_eq!(s.faces_index().faces(), 6);
    assert_eq!(index_groups(&mut s), groups_a);
    let caches: Vec<String> = crate::common::names(lib.path()).into_iter().filter(|n| n.starts_with("faces-")).collect();
    assert_eq!(caches, [format!("faces-{RECOGNISER_A}.cache"), format!("faces-{RECOGNISER_B}.cache")]);
}

#[test]
fn turning_finding_faces_off_stops_the_scan_and_can_drop_the_cached_data() {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let models = models(disk.path(), false);
    let folder = disk.path().join("Lisbon");
    write_photos(&folder, &[photo(1, RED, BLUE), photo(2, RED, BLUE)]);
    let mut s = open(lib.path());
    watch(&mut s, &[&folder]);
    scan(&mut s, &models);
    let off = s.execute("faces.enable", &json!({ "on": false })).unwrap();
    assert_eq!((off["on"].clone(), off["dropped"].clone(), off["offer"].as_str().is_some()), (json!(false), json!(false), true));
    assert_eq!(index(&mut s)["state"], json!("off"));
    assert!(!s.settings().people().find_faces);
    let caches = |lib: &Path| crate::common::names(lib).into_iter().filter(|n| n.starts_with("faces-")).count();
    assert_eq!(caches(lib.path()), 1);
    s.execute("faces.enable", &json!({ "on": false, "dropEmbeddings": true })).unwrap();
    assert!(s.wait_faces(WAIT));
    assert_eq!((caches(lib.path()), s.faces_index().faces()), (0, 0), "the cached data is gone; the faces stay");
    assert_eq!(s.execute("faces.list", &json!({})).unwrap()["total"], json!(4));
    assert_eq!(s.execute("faces.enable", &json!({ "on": true, "dropEmbeddings": true })).unwrap_err().code(), "bad_params");
    s.execute("faces.enable", &json!({ "on": true })).unwrap();
    assert!(s.wait_faces(WAIT));
    assert_eq!((stat(&mut s, "read"), s.faces_index().faces()), (4, 4), "on again: embedded again, nothing found twice");
    assert_eq!(s.execute("faces.list", &json!({})).unwrap()["total"], json!(4));
}

/// Every record logged while the capture is on: the targets of the face code, and whether any
/// record anywhere held the watched name.
struct Capture {
    face: Mutex<Vec<String>>,
    name_seen: Mutex<bool>,
}

const NAME: &str = "Zephyrine Quillfeather";

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        let text = format!("{}", record.args());
        if text.contains(NAME) || text.contains("Zephyrine") {
            *self.name_seen.lock().unwrap_or_else(PoisonError::into_inner) = true;
        }
        let t = record.target();
        if t.contains("face") || t.starts_with("dalica_faces") {
            self.face.lock().unwrap_or_else(PoisonError::into_inner).push(format!("{t}: {text}"));
        }
    }

    fn flush(&self) {}
}

fn capture() -> &'static Capture {
    static CAPTURE: OnceLock<&'static Capture> = OnceLock::new();
    CAPTURE.get_or_init(|| {
        let c: &'static Capture = Box::leak(Box::new(Capture { face: Mutex::new(Vec::new()), name_seen: Mutex::new(false) }));
        assert!(log::set_logger(c).is_ok(), "no other logger in this test binary");
        log::set_max_level(log::LevelFilter::Trace);
        c
    })
}

#[test]
fn a_trace_log_holds_no_name_rectangle_or_embedding() {
    let capture = capture();
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let models = models(disk.path(), false);
    let folder = disk.path().join("Lisbon");
    write_photos(&folder, &[photo(1, RED, BLUE), photo(2, RED, BLUE)]);
    let mut s = Session::open_with(lib.path(), Arc::new(SystemClock), &crate::common::options()).unwrap();
    let photos = watch(&mut s, &[&folder]);
    scan(&mut s, &models);
    // Name a group, ignore a face, find faces again, switch off and on.
    let groups = index_groups(&mut s);
    s.execute("faces.name", &json!({ "faces": groups[0], "name": NAME, "create": true })).unwrap();
    s.execute("faces.ignore", &json!({ "faces": [groups[1][0]] })).unwrap();
    let ids: Vec<PhotoId> = photos.values().copied().collect();
    s.execute("faces.reset", &json!({ "ids": ids })).unwrap();
    assert!(s.wait_faces(WAIT));
    s.execute("faces.enable", &json!({ "on": false })).unwrap();
    s.execute("faces.enable", &json!({ "on": true })).unwrap();
    assert!(s.wait_faces(WAIT));
    s.execute("people.list", &json!({})).unwrap();
    s.flush().unwrap();

    assert!(!*capture.name_seen.lock().unwrap(), "no record held the name");
    let face = capture.face.lock().unwrap().clone();
    assert!(face.iter().any(|l| l.contains("faces_scan")), "the scan logged: {face:?}");
    // The rectangles' corners and the embeddings' components, as a log would print them.
    let mut numbers: Vec<String> = Vec::new();
    for id in &ids {
        for f in s.library().faces(*id) {
            numbers.extend([f.rect.x0(), f.rect.y0(), f.rect.x1(), f.rect.y1()].iter().map(|v| v.to_string()));
            if let Some(v) = s.faces_index().embedding(s.library().photo(*id).unwrap(), &f.rect) {
                numbers.extend(v.unpack().iter().map(|c| format!("{c:.3}")));
            }
        }
    }
    for line in &face {
        for n in &numbers {
            assert!(!line.contains(n.as_str()), "{line:?} holds {n}");
        }
        assert!(!line.contains("[0.") && !line.contains("rect"), "{line:?}");
    }
}
