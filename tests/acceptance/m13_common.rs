//! What M13's live parts share (`m13.rs`): a loopback proxy that counts every connection the app
//! makes, the synthetic face models (a detector planted in the models folder, a recogniser
//! installed from a local file through the face model dialog), photos with painted faces, the
//! launch that puts every network connection through the proxy, and the waits for the face scan.
//!
//! **The network.** The app's HTTPS transport goes through the proxy of Options ▸ Network, by
//! default "system": `HTTPS_PROXY` (`docs/security.md` §8, `fetch::proxy`). Each launch sets it to
//! [`Proxy`], a listener on 127.0.0.1 that records the `CONNECT host:port` of every connection and
//! refuses it (`403`), so no byte ever leaves the machine and every connection the app tries,
//! whatever it is for (a tile, a place search, a model download), is counted with its host.
//!
//! **The models** (`faces.md` §3; never a real one): the detector is a network of YuNet's output
//! shape whose outputs are planted constants (two faces on every 4:3 photo, left and right, as
//! `crates/engine/tests/face_scan.rs` plants them). A detector can only be installed when Dalica
//! knows its file (`known::recognise`), so it is planted in the models folder as an install leaves
//! one (`<id>/model.onnx` and `face-model.json`). The recogniser is installed the user's way: Add a
//! Model File… on a local `.onnx`, the dialog's terms, the box ticked, Add Model. It averages a
//! face's colours into eight numbers, so faces painted alike embed alike and the groups follow the
//! paint; Dalica does not know it, so it uses its stated assumptions (`matchCosine` 0.4, the
//! Suggestion and Grouping thresholds at 85).

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dalica_faces::graph::{Attr, Graph, Node, Weight};
use dalica_faces::manifest::ModelManifest;
use dalica_faces::synthetic::{encode, len_field, node_bytes, value_info_bytes, varint_field, weight_bytes};
use dalica_image::Rgba8;
use dalica_image::encode::{self, Image, Metadata, Subsampling};
use serde_json::{Value, json};

use crate::app::{self, dialog, idle};
use crate::live::{Client, Launch};

pub const LARGE: (u32, u32) = (1400, 900);
/// How long a wait for the face scan or a file may take.
pub const PATIENCE: Duration = Duration::from_secs(300);

// --- the proxy ----------------------------------------------------------------------------------

/// A loopback HTTP proxy that refuses every `CONNECT` and remembers its target.
pub struct Proxy {
    pub port: u16,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Proxy {
    pub fn start() -> Proxy {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let log = log.clone();
                std::thread::spawn(move || answer(stream, &log));
            }
        });
        Proxy { port, seen }
    }

    /// Every connection so far: the `CONNECT` target (`host:port`), or what the first line said.
    pub fn connections(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }

    pub fn count(&self) -> usize {
        self.seen.lock().unwrap().len()
    }

    /// Waits (at most 30 s) until a connection beyond the first `before` arrives; the new ones.
    pub fn wait_beyond(&self, before: usize) -> Vec<String> {
        let end = Instant::now() + Duration::from_secs(30);
        while self.count() <= before && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
        self.connections().split_off(before.min(self.count()))
    }

    /// What every launch adds so all of the app's connections come here: `HTTPS_PROXY` (and its
    /// lower-case twin) set, `NO_PROXY` empty.
    pub fn launch(&self, launch: Launch) -> Launch {
        let url = format!("http://127.0.0.1:{}", self.port);
        launch.env("HTTPS_PROXY", &url).env("https_proxy", &url).env("NO_PROXY", "").env("no_proxy", "")
    }
}

/// Reads the request's head, records its first line's target and refuses it.
fn answer(stream: TcpStream, log: &Mutex<Vec<String>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut first = String::new();
    let _ = reader.read_line(&mut first);
    let target = match first.split_whitespace().collect::<Vec<_>>()[..] {
        ["CONNECT", target, ..] => target.to_string(),
        _ => format!("(not a CONNECT) {}", first.trim()),
    };
    log.lock().unwrap().push(target);
    let mut line = String::new();
    while reader.read_line(&mut line).is_ok_and(|n| n > 0) && line.trim() != "" {
        line.clear();
    }
    let mut out = stream;
    let _ = out.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
}

// --- the models ---------------------------------------------------------------------------------

/// The detector's input side; a 4:3 photo fills its top 64 × 48.
const SIDE: usize = 64;
pub const DETECTOR: &str = "accept-detector";

/// The detector: the twelve outputs of YuNet's shape as planted constants, two faces of score 0.9
/// in the stride-8 grid at row 2, columns 1 and 5 (on a 4:3 photo `[0.0625, 0.2917, 0.3125,
/// 0.5417]` and `[0.5625, …, 0.8125, …]`), with landmarks.
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

/// Plants the detector in `models` as an install leaves a model: `<id>/model.onnx` and its
/// `face-model.json`.
pub fn plant_detector(models: &Path) {
    let bytes = detector_bytes();
    let mut m: ModelManifest = dalica_faces::known::yunet();
    m.id = DETECTOR.into();
    m.name = "Acceptance detector".into();
    m.input.width = SIDE as u32;
    m.input.height = SIDE as u32;
    m.sha256 = Some(dalica_faces::hash::sha256_hex(&bytes));
    m.size_bytes = Some(bytes.len() as u64);
    let dir = models.join(DETECTOR);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("model.onnx"), &bytes).unwrap();
    std::fs::write(dir.join("face-model.json"), serde_json::to_vec_pretty(&m).unwrap()).unwrap();
}

/// The embedding's size.
const DIM: usize = 8;

/// The recogniser: a 1 × 1 convolution taking a face's RGB to `[r, g, b, r, g, b, r, g]` (plus a
/// small bias, so no picture gives a zero vector), global average pooling and flattening, its
/// output declared `[1, 8]` as exported recognisers declare it.
pub fn recogniser_bytes() -> Vec<u8> {
    let rows: [[f32; 3]; DIM] =
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let graph = Graph {
        nodes: vec![
            Node::new("Conv", &["data", "w", "b"], "c", &[("kernel_shape", Attr::Ints(vec![1, 1]))]),
            Node::new("GlobalAveragePool", &["c"], "p", &[]),
            Node::new("Flatten", &["p"], "emb", &[("axis", Attr::Int(1))]),
        ],
        weights: [("w".to_string(), Weight::f32(&[DIM, 3, 1, 1], rows.concat())), ("b".to_string(), Weight::f32(&[DIM], vec![0.02; DIM]))]
            .into_iter()
            .collect(),
        inputs: vec![("data".into(), vec![1, 3, 112, 112])],
        outputs: vec!["emb".into()],
    };
    let mut g = Vec::new();
    for n in &graph.nodes {
        len_field(1, &node_bytes(n), &mut g);
    }
    let mut weights: Vec<_> = graph.weights.iter().collect();
    weights.sort_by(|a, b| a.0.cmp(b.0));
    for (name, w) in weights {
        len_field(5, &weight_bytes(name, w), &mut g);
    }
    len_field(11, &value_info_bytes("data", 1, &[Ok(1), Ok(3), Ok(112), Ok(112)]), &mut g);
    len_field(12, &value_info_bytes("emb", 1, &[Ok(1), Ok(DIM as u64)]), &mut g);
    let mut ops = Vec::new();
    len_field(1, b"", &mut ops);
    varint_field(2, 13, &mut ops);
    let mut model = Vec::new();
    varint_field(1, 8, &mut model);
    len_field(2, b"accept-m13", &mut model);
    len_field(7, &g, &mut model);
    len_field(8, &ops, &mut model);
    model
}

/// The models folder of `home` with the detector planted.
pub fn models(home: &Path) -> PathBuf {
    let models = home.join("models");
    plant_detector(&models);
    models
}

/// The launch of an app whose models folder is `models`, every connection through `proxy`, with
/// the people permission and the external one (adding a model file and allowing map tiles reach
/// outside Dalica, so automation needs it, `faces.md` §9; the proxy sees whatever that lets
/// through).
pub fn launch(proxy: &Proxy, models: &Path) -> Launch {
    proxy.launch(Launch::default().arg("--automation-allow-people").arg("--automation-allow-external").env("DALICA_FACE_MODELS", models))
}

/// Installs the recogniser from the local file `file` as a user does (Options ▸ People ▸ Add a
/// Model File…, the file picked, the dialog's terms, the box ticked, Add Model): Add Model stays
/// disabled until the box is ticked, nothing is installed before. Returns the model's id; the
/// face scan then starts in the background (not waited for).
pub fn install_recogniser(c: &mut Client, models: &Path, file: &Path) -> String {
    c.exec("app.preferences", json!({ "tab": "people" }));
    c.exec("faceModel.addFile", json!({}));
    c.ok("ui.filePicker", json!({ "path": file }));
    let end = Instant::now() + Duration::from_secs(60);
    let row = loop {
        c.settle();
        let rows: Vec<Value> =
            c.widgets("text:dialog.model/").into_iter().filter(|w| w["id"].as_str().is_some_and(|i| i.starts_with("text:dialog.model/"))).collect();
        if let Some(r) = rows.first() {
            break r.clone();
        }
        assert!(Instant::now() < end, "the face model dialog never showed the file: {:?}", dialog(c));
    };
    assert_eq!(dialog(c).as_deref(), Some("faceModel"), "the face model dialog");
    let id = row["id"].as_str().unwrap().trim_start_matches("text:dialog.model/").to_string();
    let label = row["label"].as_str().unwrap_or_default().to_string();
    assert!(label.contains("Unknown"), "a file Dalica does not know shows an unknown licence: {label}");
    let install = app::widget(c, "button:dialog.install").expect("Add Model");
    assert_eq!(install["enabled"], false, "Add Model waits for the box: {install}");
    let refused = c.fails("ui.clickWidget", json!({ "id": "button:dialog.install" }));
    assert_eq!(refused["code"], "disabled", "{refused}");
    c.settle();
    assert!(!models.join(&id).exists(), "nothing is installed before the terms are accepted");
    c.ok("ui.clickWidget", json!({ "id": "check:dialog.accept" }));
    c.settle();
    c.ok("ui.clickWidget", json!({ "id": "button:dialog.install" }));
    c.settle();
    let end = Instant::now() + Duration::from_secs(120);
    while !models.join(&id).join("face-model.json").is_file() {
        assert!(Instant::now() < end, "the recogniser {id} was never installed in {}", models.display());
        c.ok("ui.settle", json!({ "timeoutMs": 50 }));
    }
    id
}

// --- the face scan ------------------------------------------------------------------------------

/// `faces.index`.
pub fn scan_status(c: &mut Client) -> Value {
    c.exec("faces.index", json!({}))
}

/// Waits until the face scan has nothing left (its state `idle`), frames running meanwhile.
pub fn scan_done(c: &mut Client) -> Value {
    let end = Instant::now() + PATIENCE;
    loop {
        let s = scan_status(c);
        if s["state"] == "idle" && s["model"].is_string() {
            idle(c);
            return s;
        }
        assert!(Instant::now() < end, "the face scan never finished: {s}");
        c.ok("ui.settle", json!({ "timeoutMs": 50 }));
    }
}

/// The groups of unnamed faces (`faces.index {groups}`), largest first.
pub fn groups(c: &mut Client) -> Vec<Vec<String>> {
    let v = c.exec("faces.index", json!({ "groups": true }));
    serde_json::from_value(v["groupList"].clone()).unwrap_or_else(|e| panic!("faces.index groups ({e}): {v}"))
}

/// `faces.list` with `params`: its faces.
pub fn faces(c: &mut Client, params: Value) -> Vec<Value> {
    let r = c.exec("faces.list", params.clone());
    r["faces"].as_array().unwrap_or_else(|| panic!("faces.list {params}: {r}")).clone()
}

/// `people.list`'s people.
pub fn people(c: &mut Client) -> Vec<Value> {
    let r = c.exec("people.list", json!({}));
    r["people"].as_array().unwrap_or_else(|| panic!("people.list: {r}")).clone()
}

/// The person named `name` (`people.list`).
pub fn person(c: &mut Client, name: &str) -> Option<Value> {
    people(c).into_iter().find(|p| p["name"] == name)
}

/// The library's undo depth and the label of its next undo (`library.inspect` → `undo`).
pub fn undo_state(c: &mut Client) -> (u64, String) {
    let r = c.exec("library.inspect", json!({}));
    (r["undo"]["depth"].as_u64().unwrap_or_else(|| panic!("library.inspect undo: {r}")), r["undo"]["undo"].as_str().unwrap_or_default().to_string())
}

// --- photos -------------------------------------------------------------------------------------

pub type Rgb = [u8; 3];

/// A 4:3 JPEG of `w × h` with a face painted in `left` and one in `right`, each well around the box
/// the detector reports there, over a soft procedural gradient.
pub fn faces_jpeg(w: u32, h: u32, left: Rgb, right: Rgb) -> Vec<u8> {
    assert_eq!(w * 3, h * 4, "a 4:3 photo fills the detector's input as planned");
    let (y0, y1) = (h * 44 / 192, h * 116 / 192);
    let (lx0, lx1, rx0, rx1) = (0, w * 96 / 256, w * 128 / 256, w * 224 / 256);
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let px = if (y0..y1).contains(&y) && (lx0..lx1).contains(&x) {
                left
            } else if (y0..y1).contains(&y) && (rx0..rx1).contains(&x) {
                right
            } else {
                [(90 + x * 60 / w) as u8, (110 + y * 50 / h) as u8, (140 + (x + y) * 30 / (w + h)) as u8]
            };
            data.extend_from_slice(&[px[0], px[1], px[2], 255]);
        }
    }
    encode::jpeg(&Image::rgba8(&Rgba8 { width: w, height: h, data }), 95, Subsampling::S444, &Metadata::default()).unwrap()
}

/// The face of `photo` on the `left` (or the right) of the list `faces`.
pub fn face_on(faces: &[Value], photo: &Value, left: bool) -> String {
    faces
        .iter()
        .find(|f| f["photo"] == *photo && (f["rect"][0].as_f64().unwrap_or(1.0) < 0.5) == left)
        .and_then(|f| f["id"].as_str().map(str::to_string))
        .unwrap_or_else(|| panic!("no {} face on photo {photo}: {faces:?}", if left { "left" } else { "right" }))
}

/// Screenshots of the window until two in a row are the same picture (the face tiles and
/// thumbnails in), then matched against `fingerprints/<name>.json` with the moving parts masked.
pub fn steady_shot(c: &mut Client, home: &Path, name: &str) {
    let mut last: Option<Vec<u8>> = None;
    for _ in 0..30 {
        idle(c);
        app::quiet(c, f64::from(LARGE.1));
        let shot = crate::live::screenshot(c, home, None, &format!("{name}-probe"), LARGE.0, LARGE.1);
        if last.as_ref() == Some(&shot.rgba) {
            break;
        }
        last = Some(shot.rgba);
        c.ok("ui.wait", json!({ "ms": 300 }));
    }
    let masks = app::moving_parts(c);
    app::masked_shot(c, home, name, LARGE, &masks);
}

// --- the log ------------------------------------------------------------------------------------

/// The run's own log lines (standard error, `RUST_LOG` as launched), kept for [`privacy`].
pub fn log_of(home: &Path) -> String {
    std::fs::read_to_string(home.join("stderr.log")).unwrap_or_default()
}

/// Whether `line` holds `word` as a word (not inside a longer one).
fn holds_word(line: &str, word: &str) -> bool {
    line.match_indices(word).any(|(i, _)| {
        let before = line[..i].chars().next_back();
        let after = line[i + word.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// The trace log of a people workflow (`faces.md` §7): no line holds a person's name or nickname
/// (`names`) or a face's corner as it would print (read from the `folders`' sidecars, the app
/// having quit), and the face code's lines (their target holds "face") hold no rectangle and no
/// vector.
pub fn privacy(logs: &str, names: &[&str], folders: &[PathBuf]) {
    let lines = logs.lines().count();
    assert!(lines > 100, "the trace log has the run's lines ({lines})");
    let mut corners: Vec<String> = Vec::new();
    for folder in folders {
        let v: Value = serde_json::from_slice(&std::fs::read(folder.join(".dalica.json")).unwrap()).unwrap();
        for (_, p) in v["photos"].as_object().into_iter().flatten() {
            for f in p["faces"].as_array().into_iter().flatten() {
                for x in f["rect"].as_array().into_iter().flatten() {
                    let text = x.as_f64().unwrap().to_string();
                    if text.len() >= 6 {
                        corners.push(text);
                    }
                }
            }
        }
    }
    assert!(!corners.is_empty(), "the sidecars hold the faces");
    for l in logs.lines() {
        for name in names {
            assert!(!holds_word(l, name), "a log line holds the name {name:?}: {l}");
        }
        for x in &corners {
            assert!(!l.contains(x.as_str()), "a log line holds a face's corner ({x}): {l}");
        }
    }
    let face_lines: Vec<&str> = logs.lines().filter(|l| l.split_whitespace().nth(2).is_some_and(|t| t.contains("face"))).collect();
    assert!(!face_lines.is_empty(), "the face code logged");
    for l in &face_lines {
        assert!(!l.contains("[0.") && !l.contains("[-0.") && !l.contains("rect"), "a face log line holds a rectangle or a vector: {l}");
    }
    println!("  the trace log ({lines} lines, {} from the face code) holds no name, nickname, rectangle or vector", face_lines.len());
}
