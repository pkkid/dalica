//! Face models as data (`faces.md` §3): manifests and their validation, the user's `catalog.json`,
//! the known models and the built-in list, a dropped file recognised by its hash or given stated
//! assumptions, the self-test before an install, and (with `DALICA_FACE_MODELS` set) the real
//! models' self-tests. No model is ever downloaded: every network is a seeded synthetic one.
//!
//! Adapted from LightCraft `crates/faces/src/manifest.rs`, `catalog.rs`, `known.rs`,
//! `suggest.rs`, `hash.rs` and `runtime.rs` tests @ f34c7a8a (MIT OR Apache-2.0).

use dalica_faces::catalog::{self, MAX_CATALOG_BYTES, MAX_ENTRIES};
use dalica_faces::graph::{Graph, Node, Weight};
use dalica_faces::hash::{sha256_file, sha256_hex, sha256_reader};
use dalica_faces::known::{self, OFFERED, Recognition};
use dalica_faces::manifest::{
    self, Colour, Commercial, InputSpec, Licence, MAX_MANIFEST_BYTES, ManifestError, ModelManifest, OutputSpec, Resize, Role, Thresholds,
};
use dalica_faces::onnx::probe_path;
use dalica_faces::selftest;
use dalica_faces::synthetic::{self, encode};
use serde_json::{Value, json};

/// A change to a manifest, named.
type Case = (&'static str, Box<dyn Fn(&mut ModelManifest)>);

fn good() -> ModelManifest {
    ModelManifest {
        id: "example-r50".into(),
        name: "Example R50".into(),
        version: "1".into(),
        role: Role::Embedder,
        licence: Licence { name: "MIT".into(), commercial: Commercial::Yes, url: Some("https://example.org/licence".into()), notice: "MIT.".into() },
        source: Some("https://example.org/model".into()),
        sha256: Some("a".repeat(64)),
        size_bytes: Some(1234),
        provenance: "synthetic".into(),
        input: InputSpec::default(),
        output: OutputSpec::Embedding { dim: 512 },
        thresholds: Thresholds { match_cosine: Some(0.4), ..Default::default() },
        calibration: Some([0.30, 0.31, 0.32, 0.33, 0.35, 0.37, 0.38, 0.40, 0.43, 0.47]),
    }
}

#[test]
fn a_manifest_without_a_licence_or_with_an_http_address_is_refused() {
    let m = good();
    assert_eq!(manifest::validate(&m), Ok(()));
    let json = serde_json::to_vec(&m).unwrap();
    assert_eq!(manifest::parse(&json).unwrap(), m, "a manifest round-trips through JSON");
    assert!(m.calibration().is_some());

    let mut no_licence: Value = serde_json::from_slice(&json).unwrap();
    no_licence.as_object_mut().unwrap().remove("licence");
    let refused = manifest::parse(no_licence.to_string().as_bytes()).unwrap_err();
    assert!(matches!(refused, ManifestError::Field("licence.name", _)), "{refused}");
    let mut no_notice = m.clone();
    no_notice.licence.notice.clear();
    assert!(manifest::validate(&no_notice).is_err(), "a licence without its terms");

    for (what, change) in [
        ("http source", Box::new(|m: &mut ModelManifest| m.source = Some("http://example.org/m.onnx".into())) as Box<dyn Fn(&mut ModelManifest)>),
        ("http licence", Box::new(|m| m.licence.url = Some("http://example.org/licence".into()))),
        ("file source", Box::new(|m| m.source = Some("file:///etc/passwd".into()))),
        ("no host", Box::new(|m| m.source = Some("https:///m.onnx".into()))),
        ("space in url", Box::new(|m| m.source = Some("https://a b".into()))),
    ] {
        let mut bad = m.clone();
        change(&mut bad);
        assert!(manifest::validate(&bad).is_err(), "{what} must be refused");
    }
}

#[test]
fn every_manifest_field_is_bounded() {
    let cases: Vec<Case> = vec![
        ("empty id", Box::new(|m| m.id = String::new())),
        ("uppercase id", Box::new(|m| m.id = "Bad".into())),
        ("path in id", Box::new(|m| m.id = "../etc".into())),
        ("dot id", Box::new(|m| m.id = ".".into())),
        ("hidden id", Box::new(|m| m.id = ".hidden".into())),
        ("slash id", Box::new(|m| m.id = "a/b".into())),
        ("backslash id", Box::new(|m| m.id = r"a\b".into())),
        ("long id", Box::new(|m| m.id = "a".repeat(65))),
        ("empty name", Box::new(|m| m.name = String::new())),
        ("control in name", Box::new(|m| m.name = "a\u{7}b".into())),
        ("huge notice", Box::new(|m| m.licence.notice = "x".repeat(801))),
        ("short sha", Box::new(|m| m.sha256 = Some("abc".into()))),
        ("uppercase sha", Box::new(|m| m.sha256 = Some("A".repeat(64)))),
        ("zero size", Box::new(|m| m.size_bytes = Some(0))),
        ("huge size", Box::new(|m| m.size_bytes = Some(u64::MAX))),
        ("tiny input", Box::new(|m| m.input.width = 4)),
        ("huge input", Box::new(|m| m.input.height = 1 << 20)),
        ("nan mean", Box::new(|m| m.input.mean[1] = f32::NAN)),
        ("zero std", Box::new(|m| m.input.std[2] = 0.0)),
        ("zero dim", Box::new(|m| m.output = OutputSpec::Embedding { dim: 0 })),
        ("huge dim", Box::new(|m| m.output = OutputSpec::Embedding { dim: 10_000_000 })),
        ("role mismatch", Box::new(|m| m.role = Role::Detector)),
        (
            "unknown decoder",
            Box::new(|m| {
                m.role = Role::Detector;
                m.output = OutputSpec::Detector { decoder: "made-up".into() };
                m.calibration = None;
            }),
        ),
        (
            "a detector's input not square",
            Box::new(|m| {
                *m = known::yunet();
                m.input.height = 480;
            }),
        ),
        ("nan threshold", Box::new(|m| m.thresholds.match_cosine = Some(f32::NAN))),
        ("threshold range", Box::new(|m| m.thresholds.score = Some(1.5))),
        ("falling calibration", Box::new(|m| m.calibration = Some([0.5, 0.4, 0.4, 0.4, 0.4, 0.4, 0.4, 0.4, 0.4, 0.4]))),
        ("calibration past 1", Box::new(|m| m.calibration = Some([2.0; 10]))),
        (
            "a detector's calibration",
            Box::new(|m| {
                *m = known::yunet();
                m.calibration = Some([0.5; 10]);
            }),
        ),
    ];
    for (what, f) in cases {
        let mut m = good();
        f(&mut m);
        assert!(manifest::validate(&m).is_err(), "{what} must be refused");
    }
    // hostile JSON
    assert!(matches!(manifest::parse(&vec![b' '; MAX_MANIFEST_BYTES + 1]), Err(ManifestError::TooLarge)));
    for bytes in
        [&b""[..], b"null", b"[]", b"{", b"{\"id\": 1}", b"\xff\xfe\x00", b"{\"id\":\"x\",\"name\":\"n\",\"version\":\"1\",\"role\":\"embedder\"}"]
    {
        assert!(manifest::parse(bytes).is_err(), "{:?}", String::from_utf8_lossy(bytes));
    }
}

#[test]
fn a_recogniser_without_a_table_uses_its_match_cosine() {
    let mut m = good();
    m.calibration = None;
    let c = m.calibration().unwrap();
    let at = |v| c.threshold(dalica_faces::calibration::Confidence::new(v).unwrap());
    assert!((at(85) - 0.4).abs() < 1e-6 && at(95) > at(85));
    m.thresholds.match_cosine = None;
    assert!(m.calibration().is_none(), "nothing to go on");
    assert!(known::yunet().calibration().is_none(), "a detector has none");
}

fn entry(id: &str) -> Value {
    json!({
        "id": id, "name": "Test model", "version": "1", "role": "embedder",
        "url": "https://example.org/w/recogniser-r50.onnx?download=1",
        "sha256": "ab".repeat(32), "sizeBytes": 1234,
        "licence": {"name": "Research only", "commercial": "no", "notice": "Not for commercial use."},
        "output": {"kind": "embedding", "dim": 512}, "thresholds": {"matchCosine": 0.4},
    })
}

fn catalog_of(models: Vec<Value>) -> catalog::Catalog {
    catalog::parse(json!({ "models": models }).to_string().as_bytes())
}

#[test]
fn a_catalog_lists_the_users_models_and_reports_the_bad_ones() {
    let c = catalog_of(vec![entry("my-model")]);
    assert_eq!(c.errors, Vec::<String>::new());
    let e = &c.entries[0];
    assert_eq!((e.manifest.id.as_str(), e.file_name.as_str()), ("my-model", "recogniser-r50.onnx"), "the file name comes from the address");
    assert_eq!(e.manifest.licence.commercial, Commercial::No, "a non-commercial model is the user's own to add");
    assert_eq!(e.manifest.input, InputSpec::default(), "the input defaults to 112 × 112 RGB, (x − 127.5) / 127.5");
    let d = e.download().unwrap();
    assert_eq!((d.size_bytes, d.sha256.len(), d.host()), (1234, 64, "example.org"));

    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend_from_slice(json!({ "models": [entry("with-bom")] }).to_string().as_bytes());
    assert_eq!(catalog::parse(&bom).entries.len(), 1, "a byte-order mark is skipped");

    let mut no_hash = entry("no-hash");
    no_hash.as_object_mut().unwrap().remove("sha256");
    let mut http = entry("plain-http");
    http["url"] = json!("http://example.org/m.onnx");
    let mut no_licence = entry("no-licence");
    no_licence.as_object_mut().unwrap().remove("licence");
    let mut detector = entry("a-detector");
    detector["role"] = json!("detector");
    detector["output"] = json!({"kind": "detector", "decoder": "yunet-v2"});
    let mut path_id = entry("x");
    path_id["id"] = json!("../escape");
    let mut known_file = entry("known-file");
    known_file["sha256"] = json!(known::SFACE_SHA256);
    let c =
        catalog_of(vec![entry("fine"), no_hash, http, no_licence, detector, path_id, entry(known::SFACE_ID), known_file, entry("fine"), json!(7)]);
    assert_eq!(c.entries.iter().map(|e| e.manifest.id.as_str()).collect::<Vec<_>>(), ["fine"]);
    assert_eq!(c.errors.len(), 9, "{:#?}", c.errors);
    assert!(c.errors.iter().any(|e| e.contains("`no-hash`") && e.contains("sha256")));
    assert!(c.errors.iter().any(|e| e.contains("`plain-http`") && e.contains("https")));
    assert!(c.errors.iter().any(|e| e.contains("`no-licence`") && e.contains("licence")));
    assert!(c.errors.iter().any(|e| e.contains("already knows")));
    assert!(c.errors.iter().any(|e| e.contains("listed twice")));

    for bytes in [&b""[..], b"not json", b"[]", b"{}", b"{\"models\": 5}", b"\xff\xfe\x00", &vec![b' '; MAX_CATALOG_BYTES + 1]] {
        let c = catalog::parse(bytes);
        assert!(c.entries.is_empty() && !c.errors.is_empty());
    }
    let many: Vec<Value> = (0..MAX_ENTRIES + 5).map(|i| entry(&format!("m{i}"))).collect();
    let c = catalog_of(many);
    assert!(c.entries.len() == MAX_ENTRIES && c.errors.iter().any(|e| e.contains("first")));
    for url in ["https://x.org/", "https://x.org/..%2f..%2fevil.onnx", "https://x.org/q.bin", "https://x.org/.onnx"] {
        let mut odd = entry("odd");
        odd["url"] = json!(url);
        let name = catalog_of(vec![odd]).entries.remove(0).file_name;
        assert!(!name.contains(['/', '\\']) && name.ends_with(".onnx") && !name.starts_with('.'), "{url} -> {name}");
    }
}

#[test]
fn known_models_are_valid_and_the_built_in_list_is_pinned_and_gated() {
    let all = known::all();
    for m in &all {
        assert_eq!(manifest::validate(m), Ok(()), "{}", m.id);
        assert!(!m.provenance.is_empty(), "{}: what it was trained on, or that it is not known", m.id);
    }
    let mut ids: Vec<_> = all.iter().map(|m| m.id.as_str()).collect();
    let mut hashes: Vec<_> = all.iter().filter_map(|m| m.sha256.as_deref()).collect();
    ids.sort_unstable();
    ids.dedup();
    hashes.sort_unstable();
    hashes.dedup();
    assert_eq!((ids.len(), hashes.len()), (all.len(), all.len()), "unique ids and hashes");
    // every candidate's address is https, pinned to a commit, and names its file
    for m in &all {
        let d = known::pinned(&m.id).unwrap();
        assert!(manifest::valid_https(&d.url), "{}", d.url);
        let commit = d.url.split('/').any(|part| part.len() == 40 && part.bytes().all(|b| b.is_ascii_hexdigit()));
        assert!(commit && !d.url.contains("/main/") && !d.url.contains("/master/"), "{}: pinned to a commit", d.id);
        assert!(d.url.ends_with(&format!("/{}", d.file_name)) && !d.file_name.contains(['/', '\\']), "{}", d.url);
        assert_eq!((Some(d.size_bytes), Some(d.sha256.as_str())), (m.size_bytes, m.sha256.as_deref()));
    }
    // only what reserved decision 5 offers, and never a non-commercial model
    for m in &all {
        let offered = OFFERED.contains(&m.id.as_str()) && m.licence.commercial != Commercial::No;
        assert_eq!(known::download(&m.id).is_some(), offered, "{}", m.id);
    }
    assert!(OFFERED.iter().all(|id| all.iter().any(|m| m.id == *id)), "every offered id is a known model");
    assert_eq!(known::builtin().len(), OFFERED.len());
    for id in ["nope", "", "../sface-2021dec", "SFACE-2021DEC"] {
        assert!(known::download(id).is_none() && known::pinned(id).is_none(), "{id}");
    }
    assert_eq!(known::pinned(known::YUNET_ID).unwrap().host(), "github.com");
    assert_eq!(known::pinned(known::AURAFACE_ID).unwrap().host(), "huggingface.co");
    assert_eq!(known::sface().licence.commercial, Commercial::Unknown, "an unresolved licence says so");
}

#[test]
fn hashes_are_lowercase_hex_sha256() {
    assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    let big = vec![7u8; 3 << 20];
    assert_eq!(sha256_reader(&big[..]).unwrap(), sha256_hex(&big), "streamed in several reads");
}

fn write(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn a_dropped_file_is_recognised_by_its_hash_or_given_stated_assumptions() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = synthetic::embedder_model(512);
    let path = write(&dir, "My Model (r50).onnx", &bytes);
    let sha = sha256_file(&path).unwrap();
    let info = probe_path(&path).unwrap();
    let size = bytes.len() as u64;

    // a known hash wins, whatever the shapes
    assert!(matches!(known::recognise(&info, known::AURAFACE_SHA256, size, "x.onnx", &[]), Recognition::Known(m) if m.id == known::AURAFACE_ID));
    assert_eq!(known::lookup(known::YUNET_SHA256).map(|m| m.role), Some(Role::Detector));
    // the user's catalog names this very file
    let mut mine = good();
    mine.id = "my-model".into();
    mine.sha256 = Some(sha.clone());
    assert!(matches!(known::recognise(&info, &sha, size, "renamed.onnx", &[mine]), Recognition::Known(m) if m.id == "my-model"));

    // unknown: a draft that validates, with its assumptions in plain words
    let Recognition::Assumed { manifest, assumptions } = known::recognise(&info, &sha, size, "My Model (r50).onnx", &[]) else {
        panic!("expected stated assumptions");
    };
    assert_eq!(manifest::validate(&manifest), Ok(()));
    assert_eq!(manifest.id, format!("custom-my-model--r50-{}", &sha[..8]));
    assert_eq!((manifest.output.clone(), manifest.input), (OutputSpec::Embedding { dim: 512 }, InputSpec::default()));
    assert_eq!((manifest.licence.commercial, manifest.sha256.as_deref()), (Commercial::Unknown, Some(sha.as_str())));
    assert!(assumptions.iter().any(|a| a.contains("112 × 112")), "{assumptions:?}");
    assert!(assumptions.iter().any(|a| a.contains("(x − 127.5) / 127.5")), "{assumptions:?}");
    assert!(assumptions.iter().any(|a| a.contains("512 numbers")), "{assumptions:?}");
    // odd file names never break the draft
    for name in ["", ".onnx", "../../etc/passwd", "\u{7}\u{0}x.onnx", &"x".repeat(5000), "ÅÄÖ 🙂.onnx"] {
        let Recognition::Assumed { manifest, .. } = known::recognise(&info, "", 0, name, &[]) else { panic!("{name:?}") };
        assert_eq!(manifest::validate(&manifest), Ok(()), "{name:?}");
    }
    // a detector-shaped or otherwise odd file is refused with the reason
    let detector = write(&dir, "detector.onnx", &synthetic::tiny_detector_model(64));
    let info = probe_path(&detector).unwrap();
    assert!(
        matches!(known::recognise(&info, &sha256_file(&detector).unwrap(), 1, "d.onnx", &[]), Recognition::Unsupported(why) if why.contains("outputs"))
    );
    let tiny = write(&dir, "tiny.onnx", &synthetic::embedder_model(2));
    let info = probe_path(&tiny).unwrap();
    assert!(matches!(known::recognise(&info, &sha, 1, "t.onnx", &[]), Recognition::Unsupported(_)), "a vector of two numbers");
}

fn embedder_manifest(dim: u32) -> ModelManifest {
    let mut m = good();
    m.sha256 = None;
    m.calibration = None;
    m.output = OutputSpec::Embedding { dim };
    m
}

#[test]
fn the_self_test_passes_a_model_that_matches_and_refuses_one_that_does_not() {
    let bytes = synthetic::tiny_embedder_model(16);
    let pass = selftest::run(&bytes, &embedder_manifest(16));
    assert!(pass.ok, "{pass:?}");
    assert!(pass.checks.len() >= 6 && pass.load_ms >= 0.0, "{pass:?}");

    let mut hashed = embedder_manifest(16);
    hashed.sha256 = Some(sha256_hex(&bytes));
    assert!(selftest::run(&bytes, &hashed).ok, "the file the manifest names");

    // the manifest says 128 numbers, the model gives 16
    let wrong_dim = selftest::run(&bytes, &embedder_manifest(128));
    assert!(!wrong_dim.ok && wrong_dim.failure().is_some_and(|f| f.contains("shapes")), "{wrong_dim:?}");
    // the manifest says 96 × 96 faces, the model reads 112 × 112
    let mut wrong_input = embedder_manifest(16);
    wrong_input.input.width = 96;
    wrong_input.input.height = 96;
    assert!(!selftest::run(&bytes, &wrong_input).ok);
    // a detector's manifest for a recogniser, another file than the manifest names, junk
    let mut detector = known::yunet();
    detector.sha256 = None;
    assert!(!selftest::run(&bytes, &detector).ok);
    let mut other = embedder_manifest(16);
    other.sha256 = Some("0".repeat(64));
    assert!(selftest::run(&bytes, &other).failure().is_some_and(|f| f.contains("SHA-256")));
    for junk in [&b""[..], b"nonsense", &bytes[..bytes.len() / 2]] {
        assert!(!selftest::run(junk, &embedder_manifest(16)).ok);
    }
    let mut invalid = embedder_manifest(16);
    invalid.licence = Licence::default();
    assert!(!selftest::run(&bytes, &invalid).ok, "a manifest without a licence");
    // a model whose every face is the same vector
    let constant = Graph {
        nodes: vec![Node::new("Identity", &["v"], "emb", &[])],
        weights: [("v".to_string(), Weight::f32(&[1, 16], (0..16).map(|i| i as f32 + 1.0).collect()))].into_iter().collect(),
        inputs: vec![("data".into(), vec![1, 3, 112, 112])],
        outputs: vec!["emb".into()],
    };
    let same = selftest::run(&encode(&constant), &embedder_manifest(16));
    assert!(!same.ok && same.failure().is_some_and(|f| f.contains("different")), "{same:?}");

    // a detector: the decoder's outputs at the manifest's size, or not
    let mut d = known::yunet();
    d.sha256 = None;
    d.size_bytes = None;
    d.input = InputSpec { width: 64, height: 64, colour: Colour::Bgr, mean: [0.0; 3], std: [1.0; 3], resize: Resize::Letterbox };
    let ok = selftest::run(&synthetic::tiny_detector_model(64), &d);
    assert!(ok.ok, "{ok:?}");
    let wrong = selftest::run(&synthetic::tiny_detector_model(96), &d);
    assert!(!wrong.ok && wrong.failure().is_some_and(|f| f.contains("shapes")), "{wrong:?}");

    let missing = selftest::run_path(std::path::Path::new("/does/not/exist.onnx"), &embedder_manifest(16));
    assert!(!missing.ok);
}

/// `DALICA_FACE_MODELS=<folder>`: every model file there recognised by its hash passes its
/// self-test. Without it the test says so and passes; the corpus task pins the real files.
#[test]
fn real_models_pass_their_self_test_when_a_folder_is_given() {
    let Some(dir) = std::env::var_os("DALICA_FACE_MODELS") else {
        eprintln!("skipped: DALICA_FACE_MODELS is not set, so no real face model was self-tested");
        return;
    };
    let mut tested = 0;
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "onnx") {
            continue;
        }
        let sha = sha256_file(&path).unwrap();
        let Some(m) = known::lookup(&sha) else {
            eprintln!("{}: not a known model, not self-tested", path.display());
            continue;
        };
        let t = selftest::run_path(&path, &m);
        eprintln!("{}: ok {} | load {:.0} ms | run {:.0} ms | {:?}", m.id, t.ok, t.load_ms, t.run_ms, t.checks);
        assert!(t.ok, "{}: {:?}", m.id, t.failure());
        tested += 1;
    }
    assert!(tested > 0, "DALICA_FACE_MODELS names a folder with no known model");
}
