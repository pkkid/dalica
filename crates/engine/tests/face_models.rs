//! Face models (`roadmap.md` M13.16; `faces.md` §3 and §9): `faces.models.list`, `.inspect`,
//! `.install`, `.download`, `.cancel`, `.remove` and `.select`, over an injected transport (an
//! in-memory server, or a gate that holds the connection open) and synthetic models; never the
//! network.
//!
//! Nothing is requested or written before `acknowledged`; a download resumes after a cut or a
//! cancel and installs only after the hash and the self-test; a wrong hash deletes the file; a local
//! file installs with its stated assumptions; removing a model deletes its cached embeddings;
//! selecting another model never mixes caches; each command's contract and graceful failures.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dalica_core::SystemClock;
use dalica_engine::Session;
use dalica_engine::jobs::{JobEntry, JobState};
use dalica_faces::hash::sha256_hex;
use dalica_faces::synthetic;
use dalica_fetch::{Body, Limits, MemoryTransport, Reply, Request, Served, Transport, TransportError};
use dalica_fs::Expect;
use serde_json::{Value, json};

use crate::common::{exists, open};

const WAIT: Duration = Duration::from_secs(120);
const URL: &str = "https://models.test/weights/mine.onnx";
const MINE: &str = "my-recogniser";

/// A recogniser that passes the self-test (16 numbers per face).
fn good_model() -> Vec<u8> {
    shaped(&synthetic::tiny_embedder_model(16), 16)
}

/// `model` written again with its output declared as `[1, dim]`, as exported recognisers declare
/// it (`dalica_faces::synthetic::encode` leaves output shapes out), so an unknown file is taken
/// for a recogniser.
fn shaped(model: &[u8], dim: u64) -> Vec<u8> {
    use dalica_faces::synthetic::{len_field, node_bytes, value_info_bytes, varint_field, weight_bytes};
    let graph = dalica_faces::graph::load(model).unwrap();
    let mut g = Vec::new();
    for n in &graph.nodes {
        len_field(1, &node_bytes(n), &mut g);
    }
    let mut weights: Vec<_> = graph.weights.iter().collect();
    weights.sort_by(|a, b| a.0.cmp(b.0));
    for (name, w) in weights {
        len_field(5, &weight_bytes(name, w), &mut g);
    }
    for (name, shape) in &graph.inputs {
        let dims: Vec<Result<u64, &str>> = shape.iter().map(|&d| Ok(d as u64)).collect();
        len_field(11, &value_info_bytes(name, 1, &dims), &mut g);
    }
    for name in &graph.outputs {
        len_field(12, &value_info_bytes(name, 1, &[Ok(1), Ok(dim)]), &mut g);
    }
    let mut ops = Vec::new();
    len_field(1, b"", &mut ops);
    varint_field(2, 13, &mut ops);
    let mut out = Vec::new();
    varint_field(1, 8, &mut out);
    len_field(2, b"test", &mut out);
    len_field(7, &g, &mut out);
    len_field(8, &ops, &mut out);
    out
}

/// A `catalog.json` naming one recogniser at [`URL`] with the given pins and dimension.
fn catalog(models: &Path, sha256: &str, size: usize, dim: u32) {
    let doc = json!({ "models": [{
        "id": MINE, "name": "My recogniser", "version": "1", "role": "embedder",
        "url": URL, "sha256": sha256, "sizeBytes": size,
        "licence": { "name": "Research use only", "commercial": "no", "notice": "Not for commercial use." },
        "provenance": "Trained on a test set.",
        "output": { "kind": "embedding", "dim": dim },
        "thresholds": { "matchCosine": 0.5 }
    }]});
    dalica_fs::create_dir_all(models).unwrap();
    let path = models.join("catalog.json");
    let bytes = serde_json::to_vec_pretty(&doc).unwrap();
    match dalica_fs::read(&path, 1 << 20).unwrap() {
        Some(r) => dalica_fs::replace(&path, &bytes, Expect::Unchanged(r.identity)).unwrap(),
        None => dalica_fs::replace(&path, &bytes, Expect::Absent).unwrap(),
    };
}

/// A session on a fresh library with its models folder and `transport`.
struct Fixture {
    _dirs: (tempfile::TempDir, tempfile::TempDir),
    lib: PathBuf,
    models: PathBuf,
    disk: PathBuf,
    s: Session,
}

fn fixture(transport: Option<Arc<dyn Transport + Send + Sync>>) -> Fixture {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let models = disk.path().join("models");
    dalica_fs::create_dir_all(&models).unwrap();
    let mut s = open(lib.path());
    s.set_face_model_transport(transport);
    s.set_face_models_dir(Some(models.clone()));
    Fixture { lib: lib.path().to_path_buf(), models, disk: disk.path().to_path_buf(), s, _dirs: (lib, disk) }
}

impl Fixture {
    fn list(&mut self) -> Value {
        self.s.execute("faces.models.list", &json!({ "refresh": true })).unwrap()
    }

    fn model(&mut self, id: &str) -> Value {
        let list = self.list();
        list["models"].as_array().unwrap().iter().find(|m| m["id"] == json!(id)).cloned().unwrap_or(Value::Null)
    }

    /// Runs a command that starts a job and waits for the job.
    fn job(&mut self, id: &str, params: Value) -> JobEntry {
        let reply = self.s.execute(id, &params).unwrap();
        let job = reply["job"].as_u64().unwrap_or_else(|| panic!("{id} started no job: {reply}"));
        let entry = self.s.wait_job(job, WAIT).unwrap();
        assert!(self.s.wait_face_models(WAIT));
        entry
    }

    fn installed(&self, id: &str) -> bool {
        exists(&self.models.join(id).join("model.onnx")) && exists(&self.models.join(id).join("face-model.json"))
    }

    fn staged(&self, name: &str) -> bool {
        exists(&self.models.join(".downloads").join(name))
    }

    /// Writes `bytes` as a local model file and returns its path.
    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.disk.join(name);
        dalica_fs::replace(&path, bytes, Expect::Absent).unwrap();
        path
    }
}

fn code(r: Result<Value, dalica_engine::CmdError>) -> &'static str {
    r.unwrap_err().code()
}

#[test]
fn nothing_is_requested_or_written_before_acknowledged() {
    let bytes = good_model();
    let server = Arc::new(MemoryTransport::new());
    server.serve(URL, Served::new(bytes.clone()));
    let mut fx = fixture(Some(server.clone()));
    catalog(&fx.models, &sha256_hex(&bytes), bytes.len(), 16);
    let local = fx.file("local.onnx", &bytes);
    assert_eq!(fx.model(MINE)["state"], json!("available"));

    for ack in [json!(false), json!(null), json!("yes"), json!(1)] {
        let download = fx.s.execute("faces.models.download", &json!({ "id": MINE, "acknowledged": ack }));
        let install = fx.s.execute("faces.models.install", &json!({ "path": local, "acknowledged": ack }));
        let want = if ack == json!(false) { "disabled" } else { "bad_params" };
        assert_eq!((code(download), code(install)), (want, want), "acknowledged {ack}");
    }
    assert_eq!(code(fx.s.execute("faces.models.download", &json!({ "id": MINE }))), "bad_params", "acknowledged is required");
    assert!(fx.s.wait_face_models(WAIT));
    assert!(server.requests().is_empty(), "no request before the terms are accepted");
    assert!(fx.s.jobs().is_empty(), "no job started");
    assert!(!fx.installed(MINE) && !exists(&fx.models.join(".downloads")), "nothing written");
    let folders: Vec<_> = std::fs::read_dir(&fx.models).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(folders, ["catalog.json"], "the models folder holds only the user's catalog");
}

#[test]
fn a_download_resumes_after_a_cut_and_installs_after_the_hash_and_the_self_test() {
    let bytes = good_model();
    let server = Arc::new(MemoryTransport::new());
    // Every answer breaks after 40 bytes: three tries fetch 120.
    server.serve(URL, Served { cut_after: Some(40), chunk: 16, ..Served::new(bytes.clone()) });
    let mut fx = fixture(Some(server.clone()));
    catalog(&fx.models, &sha256_hex(&bytes), bytes.len(), 16);

    let cut = fx.job("faces.models.download", json!({ "id": MINE, "acknowledged": true }));
    assert_eq!(cut.state, JobState::Failed, "{cut:?}");
    assert!(cut.error.as_deref().unwrap().contains("reset"), "{cut:?}");
    assert_eq!(server.requests().iter().map(|r| r.from).collect::<Vec<_>>(), [0, 40, 80], "each try resumed");
    let entry = fx.model(MINE);
    assert_eq!((entry["state"].clone(), entry["partialBytes"].clone()), (json!("partial"), json!(120)), "{entry}");
    assert!(!fx.installed(MINE));

    server.serve(URL, Served::new(bytes.clone()));
    let done = fx.job("faces.models.download", json!({ "id": MINE, "acknowledged": true }));
    assert_eq!(done.state, JobState::Done, "{done:?}");
    let result = done.result.unwrap();
    assert_eq!((result["resumedFrom"].clone(), result["installed"].clone()), (json!(120), json!(true)), "{result}");
    assert_eq!(result["selfTest"]["ok"], json!(true));
    assert_eq!(server.requests().last().unwrap().from, 120, "the last request asked for the rest only");
    assert!(fx.installed(MINE));
    assert_eq!(std::fs::read(fx.models.join(MINE).join("model.onnx")).unwrap(), bytes);
    assert!(!fx.staged(&format!("{MINE}.onnx")) && !fx.staged(&format!("{MINE}.onnx.part")), "nothing left in the downloads folder");
    let entry = fx.model(MINE);
    assert_eq!((entry["state"].clone(), entry["inUse"].clone(), entry["origin"].clone()), (json!("installed"), json!(true), json!("catalog")));
    assert_eq!((entry["caution"].clone(), entry["commercialUse"].clone()), (json!(true), json!("no")), "a catalog model's notice is a caution");

    // Downloading it again requests nothing.
    let before = server.requests().len();
    let again = fx.s.execute("faces.models.download", &json!({ "id": MINE, "acknowledged": true })).unwrap();
    assert_eq!((again["already"].clone(), again["job"].clone()), (json!(true), Value::Null));
    assert_eq!(server.requests().len(), before);
}

#[test]
fn a_model_that_fails_its_self_test_is_deleted_and_never_installed() {
    let bytes = good_model();
    let server = Arc::new(MemoryTransport::new());
    server.serve(URL, Served::new(bytes.clone()));
    let mut fx = fixture(Some(server.clone()));
    // The right file, but the catalog says it gives 128 numbers: the self-test refuses it.
    catalog(&fx.models, &sha256_hex(&bytes), bytes.len(), 128);
    let failed = fx.job("faces.models.download", json!({ "id": MINE, "acknowledged": true }));
    assert_eq!(failed.state, JobState::Failed, "{failed:?}");
    assert!(failed.error.as_deref().unwrap().contains("self-test"), "{failed:?}");
    assert!(!fx.installed(MINE) && !fx.staged(&format!("{MINE}.onnx")), "deleted, never installed");
    assert_eq!(fx.model(MINE)["state"], json!("available"));
}

#[test]
fn a_wrong_hash_deletes_the_file() {
    let bytes = good_model();
    let mut other = bytes.clone();
    let last = other.len() - 1;
    other[last] ^= 0xff;
    let server = Arc::new(MemoryTransport::new());
    server.serve(URL, Served::new(other));
    let mut fx = fixture(Some(server.clone()));
    catalog(&fx.models, &sha256_hex(&bytes), bytes.len(), 16);
    let failed = fx.job("faces.models.download", json!({ "id": MINE, "acknowledged": true }));
    assert_eq!(failed.state, JobState::Failed, "{failed:?}");
    assert!(failed.error.as_deref().unwrap().contains("deleted"), "{failed:?}");
    assert!(!fx.installed(MINE));
    assert!(!fx.staged(&format!("{MINE}.onnx")) && !fx.staged(&format!("{MINE}.onnx.part")), "the wrong file is gone");
    assert_eq!(fx.model(MINE)["state"], json!("available"));
}

/// A server that sends the first `first` bytes of `bytes` and then holds the connection open until
/// the download is cancelled.
struct Gate {
    bytes: Vec<u8>,
    first: usize,
    asked: Mutex<Vec<u64>>,
    /// Raised once a held connection saw the download cancelled.
    released: Arc<std::sync::atomic::AtomicBool>,
}

impl Gate {
    fn new(bytes: Vec<u8>, first: usize) -> Gate {
        Gate { bytes, first, asked: Mutex::new(Vec::new()), released: Arc::default() }
    }
}

struct GateBody {
    bytes: Vec<u8>,
    pos: usize,
    released: Arc<std::sync::atomic::AtomicBool>,
}

impl Transport for Gate {
    fn get(&self, request: &Request, _: &Limits<'_>) -> Result<Reply, TransportError> {
        self.asked.lock().unwrap().push(request.from());
        let body = GateBody { bytes: self.bytes[..self.first].to_vec(), pos: 0, released: self.released.clone() };
        Ok(Reply { status: 200, length: Some(self.bytes.len() as u64), content_range: None, body: Box::new(body) })
    }
}

impl Body for GateBody {
    fn read(&mut self, buf: &mut [u8], limits: &Limits<'_>) -> Result<usize, TransportError> {
        if self.pos < self.bytes.len() {
            let n = buf.len().min(self.bytes.len() - self.pos);
            buf[..n].copy_from_slice(&self.bytes[self.pos..self.pos + n]);
            self.pos += n;
            return Ok(n);
        }
        while !limits.cancel.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(1));
        }
        self.released.store(true, Ordering::SeqCst);
        Err(TransportError::Cancelled)
    }
}

#[test]
fn a_cancelled_download_keeps_what_it_fetched_and_resumes() {
    let bytes = good_model();
    let gate = Arc::new(Gate::new(bytes.clone(), 100));
    let mut fx = fixture(Some(gate.clone()));
    catalog(&fx.models, &sha256_hex(&bytes), bytes.len(), 16);

    let started = fx.s.execute("faces.models.download", &json!({ "id": MINE, "acknowledged": true })).unwrap();
    let job = started["job"].as_u64().unwrap();
    assert_eq!(fx.s.face_models().download(), Some(job));
    // The same download asked for again is the one running; another cannot start beside it.
    assert_eq!(fx.s.execute("faces.models.download", &json!({ "id": MINE, "acknowledged": true })).unwrap()["job"], json!(job));
    while fx.s.job(job).unwrap().progress.done < 100 {
        fx.s.poll();
        std::thread::sleep(Duration::from_millis(1));
    }
    let running = fx.s.job(job).unwrap();
    assert_eq!((running.state, running.progress.total, running.cancellable), (JobState::Running, bytes.len() as u64, true));
    assert!(running.label.starts_with("Downloading My recogniser: "), "{}", running.label);
    let list = fx.list();
    assert_eq!((list["download"]["id"].clone(), list["download"]["done"].clone()), (json!(MINE), json!(100)));
    assert_eq!(fx.model(MINE)["state"], json!("downloading"));
    assert_eq!(code(fx.s.execute("faces.models.remove", &json!({ "id": MINE }))), "limit", "being downloaded");

    let cancelled = fx.s.execute("faces.models.cancel", &json!({ "id": MINE })).unwrap();
    assert_eq!((cancelled["cancelled"].clone(), cancelled["job"].clone()), (json!(true), json!(job)));
    let entry = fx.s.wait_job(job, WAIT).unwrap();
    assert_eq!(entry.state, JobState::Cancelled, "{entry:?}");
    assert!(fx.s.wait_face_models(WAIT));
    assert_eq!(code(fx.s.execute("faces.models.cancel", &json!({ "id": MINE }))), "not_found", "nothing runs now");
    assert!(fx.staged(&format!("{MINE}.onnx.part")), "what was fetched is kept");
    assert_eq!(fx.model(MINE)["partialBytes"], json!(100));

    let server = Arc::new(MemoryTransport::new());
    server.serve(URL, Served::new(bytes.clone()));
    fx.s.set_face_model_transport(Some(server.clone()));
    let done = fx.job("faces.models.download", json!({ "id": MINE, "acknowledged": true }));
    assert_eq!(done.state, JobState::Done, "{done:?}");
    assert_eq!(done.result.unwrap()["resumedFrom"], json!(100));
    assert_eq!(server.requests()[0].from, 100);
    assert!(fx.installed(MINE));
}

#[test]
fn closing_the_session_stops_its_download() {
    let bytes = good_model();
    let gate = Arc::new(Gate::new(bytes.clone(), 50));
    let mut fx = fixture(Some(gate.clone()));
    catalog(&fx.models, &sha256_hex(&bytes), bytes.len(), 16);
    let job = fx.s.execute("faces.models.download", &json!({ "id": MINE, "acknowledged": true })).unwrap()["job"].as_u64().unwrap();
    while fx.s.job(job).unwrap().progress.done < 50 {
        fx.s.poll();
        std::thread::sleep(Duration::from_millis(1));
    }
    let Fixture { s, models, _dirs, .. } = fx;
    drop(s);
    let until = std::time::Instant::now() + WAIT;
    while !gate.released.load(Ordering::SeqCst) {
        assert!(std::time::Instant::now() < until, "the download stopped when its session closed");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(gate.asked.lock().unwrap().len(), 1, "and asked for nothing more");
    assert!(exists(&models.join(".downloads").join(format!("{MINE}.onnx.part"))), "what it fetched is kept to resume");
}

#[test]
fn a_local_file_installs_with_its_stated_assumptions() {
    let mut fx = fixture(None);
    let path = fx.file("My Model.onnx", &good_model());
    let looked = fx.job("faces.models.inspect", json!({ "path": path }));
    assert_eq!(looked.state, JobState::Done, "{looked:?}");
    let ins = looked.result.unwrap();
    assert_eq!((ins["known"].clone(), ins["refused"].clone(), ins["installed"].clone()), (json!(false), Value::Null, json!(false)), "{ins}");
    let id = ins["model"]["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("custom-my-model-"), "{id}");
    assert_eq!((ins["model"]["origin"].clone(), ins["model"]["licence"]["commercial"].clone()), (json!("local"), json!("unknown")));
    let assumptions = ins["assumptions"].as_array().unwrap();
    assert!(assumptions.iter().any(|a| a.as_str().unwrap().contains("112 × 112")), "{ins}");
    assert!(assumptions.iter().any(|a| a.as_str().unwrap().contains("16 numbers")), "{ins}");
    assert!(!fx.installed(&id), "inspecting installs nothing");

    let added = fx.job("faces.models.install", json!({ "path": path, "acknowledged": true }));
    assert_eq!(added.state, JobState::Done, "{added:?}");
    let r = added.result.unwrap();
    assert_eq!(
        (r["id"].clone(), r["installed"].clone(), r["already"].clone(), r["known"].clone()),
        (json!(id), json!(true), json!(false), json!(false))
    );
    assert_eq!(r["assumptions"].as_array().unwrap().len(), assumptions.len(), "the same assumptions");
    assert_eq!(r["selfTest"]["ok"], json!(true));
    assert!(fx.installed(&id));
    assert!(exists(&path), "the user's file stays where it was");
    let manifest: Value = serde_json::from_slice(&std::fs::read(fx.models.join(&id).join("face-model.json")).unwrap()).unwrap();
    assert_eq!(manifest["sha256"], json!(sha256_hex(&good_model())));
    let entry = fx.model(&id);
    assert_eq!(
        (entry["state"].clone(), entry["origin"].clone(), entry["inUse"].clone(), entry["role"].clone()),
        (json!("installed"), json!("local"), json!(true), json!("recogniser"))
    );
    let looked = fx.job("faces.models.inspect", json!({ "path": path }));
    assert_eq!(looked.result.unwrap()["installed"], json!(true));
    // Adding it again changes nothing.
    let again = fx.job("faces.models.install", json!({ "path": path, "acknowledged": true }));
    assert_eq!(again.result.unwrap()["already"], json!(true));

    // An unknown detector-shaped file, or a file that is no model, is refused with the reason.
    for (name, bytes, why) in [("det.onnx", synthetic::tiny_detector_model(64), "outputs"), ("notes.onnx", b"not a model".to_vec(), "ONNX")] {
        let path = fx.file(name, &bytes);
        let looked = fx.job("faces.models.inspect", json!({ "path": path }));
        assert!(looked.result.unwrap()["refused"].as_str().unwrap().contains(why), "{name}");
        let refused = fx.job("faces.models.install", json!({ "path": path, "acknowledged": true }));
        assert_eq!(refused.state, JobState::Failed, "{name}");
        assert!(refused.error.unwrap().contains(why), "{name}");
    }
    let folders = std::fs::read_dir(&fx.models).unwrap().count();
    assert_eq!(folders, 1, "only the model added");
}

/// A library with one photo and one face drawn on it; returns the photo folder.
fn one_face(fx: &mut Fixture) {
    let folder = fx.disk.join("photos");
    dalica_fs::create_dir_all(&folder).unwrap();
    dalica_fs::replace(&folder.join("IMG_0001.jpg"), &crate::common::jpeg_of(3, 320, 240), Expect::Absent).unwrap();
    fx.s.execute("folders.add", &json!({ "path": folder })).unwrap();
    fx.s.wait_idle();
    let id = fx.s.library().photos().next().unwrap().id;
    fx.s.execute("faces.add", &json!({ "id": id, "rect": [0.3, 0.3, 0.6, 0.7] })).unwrap();
}

fn scanned(fx: &mut Fixture) -> Value {
    assert!(fx.s.wait_faces(WAIT));
    fx.s.execute("faces.index", &json!({})).unwrap()
}

#[test]
fn selecting_another_model_never_mixes_caches_and_removing_one_deletes_its_embeddings() {
    let mut fx = fixture(None);
    one_face(&mut fx);
    let mut ids = Vec::new();
    for (name, bytes) in [("a.onnx", good_model()), ("b.onnx", shaped(&synthetic::tiny_embedder_model(8), 8))] {
        let path = fx.file(name, &bytes);
        let added = fx.job("faces.models.install", json!({ "path": path, "acknowledged": true }));
        assert_eq!(added.state, JobState::Done, "{added:?}");
        ids.push(added.result.unwrap()["id"].as_str().unwrap().to_string());
    }
    let (a, b) = (ids[0].clone(), ids[1].clone());
    fx.s.execute("faces.enable", &json!({ "on": true })).unwrap();
    let lib = fx.lib.clone();
    let cache = |id: &str| lib.join(format!("faces-{id}.cache"));

    // The newest installed is in use.
    let index = scanned(&mut fx);
    assert_eq!((index["model"].clone(), index["faces"].clone()), (json!(b), json!(1)), "{index}");
    assert_eq!(fx.list()["inUse"], json!(b));
    assert_eq!(fx.s.faces_index().model_info().unwrap().dim, 8);

    let selected = fx.s.execute("faces.models.select", &json!({ "id": a })).unwrap();
    assert_eq!(selected["selected"], json!(a));
    assert_eq!(fx.s.settings().people().model.as_deref(), Some(a.as_str()));
    let index = scanned(&mut fx);
    assert_eq!((index["model"].clone(), index["faces"].clone()), (json!(a), json!(1)), "{index}");
    assert_eq!(fx.s.faces_index().model_info().unwrap().dim, 16, "only the selected model's embeddings");
    assert!(exists(&cache(&a)) && exists(&cache(&b)), "each model keeps its own cache");
    let list = fx.list();
    assert_eq!((list["selected"].clone(), list["inUse"].clone(), list["loaded"].clone()), (json!(a), json!(a), json!(a)));

    // Back to the newest: B's cached embeddings come back, A's never join them.
    fx.s.execute("faces.models.select", &json!({ "id": null })).unwrap();
    let index = scanned(&mut fx);
    assert_eq!((index["model"].clone(), index["faces"].clone()), (json!(b), json!(1)), "{index}");
    assert_eq!(fx.s.faces_index().model_info().unwrap().dim, 8);

    // Removing the model in use deletes its file and its cached embeddings; the other takes over.
    let removed = fx.job("faces.models.remove", json!({ "id": b }));
    assert_eq!(removed.state, JobState::Done, "{removed:?}");
    assert_eq!(removed.result.unwrap(), json!({ "id": b, "removed": true, "embeddingsRemoved": true }));
    assert!(!fx.installed(&b) && !exists(&cache(&b)), "the model and its embeddings are gone");
    assert!(exists(&cache(&a)), "the other model's embeddings stay");
    let index = scanned(&mut fx);
    assert_eq!((index["model"].clone(), index["faces"].clone()), (json!(a), json!(1)), "{index}");
    assert!(fx.model(&b).is_null(), "no longer listed");
    assert_eq!(fx.s.face_scan().models_dir(), Some(fx.models.as_path()), "the scan has its folder back");

    // Removing the selected model forgets the selection.
    fx.s.execute("faces.models.select", &json!({ "id": a })).unwrap();
    assert_eq!(fx.job("faces.models.remove", json!({ "id": a })).state, JobState::Done);
    assert_eq!(fx.s.settings().people().model, None);
    assert!(!exists(&cache(&a)));
    assert_eq!(scanned(&mut fx)["state"], json!("noModel"));
    assert_eq!(fx.s.library().faces(fx.s.library().photos().next().unwrap().id).len(), 1, "the face itself stays");
}

#[test]
fn the_list_shows_the_built_in_models_with_their_terms() {
    let mut fx = fixture(None);
    let list = fx.list();
    assert_eq!(list["folder"], json!(fx.models));
    assert_eq!(list["problem"], Value::Null);
    let models = list["models"].as_array().unwrap();
    let ids: Vec<_> = models.iter().map(|m| m["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["yunet-2023mar", "sface-2021dec", "auraface-v1"], "the built-in list, in its order");
    for m in models {
        assert_eq!((m["origin"].clone(), m["state"].clone(), m["inUse"].clone()), (json!("builtin"), json!("available"), json!(false)), "{m}");
        for key in ["name", "trainedOn", "page"] {
            assert!(m[key].as_str().is_some_and(|v| !v.is_empty()), "{key}: {m}");
        }
        assert!(m["licence"]["notice"].as_str().is_some_and(|n| !n.is_empty()), "{m}");
        assert!(m["sizeBytes"].as_u64().unwrap() > 100_000, "{m}");
        assert!(["github.com", "huggingface.co"].contains(&m["site"].as_str().unwrap()), "{m}");
    }
    assert_eq!(models[1]["default"], json!(true), "SFace is the default recogniser");
    assert_eq!((models[0]["role"].clone(), models[2]["commercialUse"].clone()), (json!("detector"), json!("yes")));
    assert!(list["commercialNote"].as_str().unwrap().contains("training") || list["commercialNote"].as_str().unwrap().contains("trained"));

    // A broken catalog is reported, never a reason to list nothing.
    dalica_fs::replace(&fx.models.join("catalog.json"), b"{ not json", Expect::Absent).unwrap();
    let list = fx.list();
    assert_eq!(list["models"].as_array().unwrap().len(), 3);
    assert!(list["catalogErrors"][0].as_str().unwrap().contains("not valid JSON"), "{list}");
}

#[test]
fn each_command_fails_gracefully() {
    let bytes = good_model();
    let mut fx = fixture(Some(Arc::new(MemoryTransport::new())));
    let local = fx.file("m.onnx", &bytes);
    let ack = |v: Value| {
        let mut v = v;
        v["acknowledged"] = json!(true);
        v
    };
    // Hostile or unknown ids and paths.
    for id in ["", "../x", "X", "a/b", "sface-2021dec/../..", &"x".repeat(200)] {
        assert_eq!(code(fx.s.execute("faces.models.download", &ack(json!({ "id": id })))), "bad_params", "{id:?}");
        assert_eq!(code(fx.s.execute("faces.models.remove", &json!({ "id": id }))), "bad_params", "{id:?}");
        assert_eq!(code(fx.s.execute("faces.models.select", &json!({ "id": id }))), "bad_params", "{id:?}");
        assert_eq!(code(fx.s.execute("faces.models.cancel", &json!({ "id": id }))), "not_found", "{id:?}");
    }
    assert_eq!(code(fx.s.execute("faces.models.download", &ack(json!({ "id": "unknown-model" })))), "not_found");
    assert_eq!(code(fx.s.execute("faces.models.remove", &json!({ "id": "sface-2021dec" }))), "not_found", "not installed");
    assert_eq!(code(fx.s.execute("faces.models.select", &json!({ "id": "sface-2021dec" }))), "not_found", "not installed");
    for path in [json!("relative.onnx"), json!(""), json!(5), json!(null)] {
        assert_eq!(code(fx.s.execute("faces.models.inspect", &json!({ "path": path }))), "bad_params", "{path}");
        assert_eq!(code(fx.s.execute("faces.models.install", &ack(json!({ "path": path })))), "bad_params", "{path}");
    }
    for (id, params) in [
        ("faces.models.list", json!({ "zzz": 1 })),
        ("faces.models.list", json!([])),
        ("faces.models.inspect", json!({})),
        ("faces.models.cancel", json!({})),
        ("faces.models.remove", json!({ "id": 3 })),
        ("faces.models.select", json!({ "id": [] })),
    ] {
        assert_eq!(code(fx.s.execute(id, &params)), "bad_params", "{id} {params}");
    }
    // A file that is missing fails its job, saying why.
    let missing = fx.job("faces.models.inspect", json!({ "path": fx.disk.join("gone.onnx") }));
    assert_eq!(missing.state, JobState::Failed);
    assert!(missing.error.unwrap().contains("does not exist"));
    // A detector cannot be chosen with Use.
    let det = fx.disk.join("models").join("test-detector");
    dalica_fs::create_dir_all(&det).unwrap();
    let detector = synthetic::tiny_detector_model(64);
    let mut m = dalica_faces::known::yunet();
    m.id = "test-detector".into();
    m.input.width = 64;
    m.input.height = 64;
    m.sha256 = Some(sha256_hex(&detector));
    m.size_bytes = Some(detector.len() as u64);
    dalica_fs::replace(&det.join("model.onnx"), &detector, Expect::Absent).unwrap();
    dalica_fs::replace(&det.join("face-model.json"), &serde_json::to_vec(&m).unwrap(), Expect::Absent).unwrap();
    fx.list();
    assert_eq!(code(fx.s.execute("faces.models.select", &json!({ "id": "test-detector" }))), "bad_params");
    assert_eq!(fx.model("test-detector")["origin"], json!("local"));
    assert_eq!(fx.job("faces.models.remove", json!({ "id": "test-detector" })).state, JobState::Done);
    assert!(fx.model("test-detector").is_null());

    // Without a models folder nothing can be added; the built-in list still shows.
    fx.s.set_face_models_dir(None);
    let list = fx.s.execute("faces.models.list", &json!({})).unwrap();
    assert_eq!((list["folder"].clone(), list["models"].as_array().unwrap().len()), (Value::Null, 3));
    assert!(list["problem"].as_str().unwrap().contains("no face models folder"), "{list}");
    assert_eq!(code(fx.s.execute("faces.models.install", &ack(json!({ "path": local })))), "disabled");
    assert_eq!(code(fx.s.execute("faces.models.download", &ack(json!({ "id": "sface-2021dec" })))), "disabled");

    // A models folder inside a watched folder is refused.
    let watched = fx.disk.join("photos");
    dalica_fs::create_dir_all(&watched.join("models")).unwrap();
    fx.s.execute("folders.add", &json!({ "path": watched })).unwrap();
    fx.s.wait_idle();
    fx.s.set_face_models_dir(Some(watched.join("models")));
    let refused = fx.s.execute("faces.models.install", &ack(json!({ "path": local })));
    assert!(matches!(&refused, Err(e) if e.code() == "disabled" && e.to_string().contains("inside the watched folder")), "{refused:?}");
    assert_eq!(code(fx.s.execute("faces.models.download", &ack(json!({ "id": "sface-2021dec" })))), "disabled");
    assert!(fx.list()["problem"].as_str().unwrap().contains("inside the watched folder"));
    assert!(fx.s.jobs().iter().all(|j| j.kind != dalica_engine::jobs::JobKind::Network), "nothing was downloaded");
}

#[test]
fn a_read_only_session_adds_and_removes_no_model() {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut s = Session::open_read_only(lib.path(), Arc::new(SystemClock), &crate::common::options()).unwrap();
    s.set_face_model_transport(Some(Arc::new(MemoryTransport::new())));
    s.set_face_models_dir(Some(disk.path().to_path_buf()));
    let path = disk.path().join("m.onnx");
    dalica_fs::replace(&path, &good_model(), Expect::Absent).unwrap();
    assert_eq!(code(s.execute("faces.models.install", &json!({ "path": path, "acknowledged": true }))), "read_only");
    assert_eq!(code(s.execute("faces.models.download", &json!({ "id": "sface-2021dec", "acknowledged": true }))), "read_only");
    assert_eq!(code(s.execute("faces.models.remove", &json!({ "id": "sface-2021dec" }))), "read_only");
    assert!(s.execute("faces.models.list", &json!({})).unwrap()["problem"].as_str().unwrap().contains("read-only"));
}
