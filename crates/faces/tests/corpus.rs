//! The face corpus test (`docs/faces.md` §10, `docs/development.md` §11), compiled only with the
//! `corpus` feature and run by `cargo xtask test-corpus -p dalica-faces`: the built-in models
//! (`corpus/face-models/`, fetched from their pinned upstreams) on the public-domain portraits of
//! `corpus/public-faces/` (one folder per person, several portraits each), fetched by
//! `cargo xtask corpus --all`.
//!
//! Both sets are checked against their manifests first (`xtask/corpus/<set>.sha256`): a missing or
//! stale file is a failure ("run `cargo xtask corpus --all`"), never a skip, and the model pins of
//! `xtask` must be the ones `dalica_faces::known` records. Then, for every portrait:
//!
//! - **detection:** the detector (YuNet) finds a face; the portrait's subject is its largest face;
//!   `located` counts subjects found where the recorded reference (`reference/yunet-2023mar.tsv` of
//!   the set) has them (intersection over union at least 0.9);
//! - **alignment:** the subject's five landmarks are in a face's order (eyes above the nose, the
//!   nose above the mouth, left before right, all inside the padded box) and give a template
//!   transform and a crop;
//! - **embedding**, for each built-in recogniser (SFace, AuraFace): one unit vector, which must
//!   agree with the recorded reference vector of the set (`reference/<model id>.tsv`; cosine at
//!   least 0.999);
//! - **grouping** at the default confidence (85) on the model's calibration: `people_whole` counts
//!   the people whose portraits all fall in one group, `faces_in_pure_groups` the faces whose group
//!   holds one person only (merging everything or splitting everything each fails one of them);
//! - **suggestions** at the default confidence: each face in turn hidden and asked who it looks
//!   like from everyone else's portraits (`Gallery::suggest`, the default margin): suggestions made
//!   and right, and their precision in thousandths;
//! - **calibration:** the model's table in `known.rs` must be the one measured here, by the rule
//!   [`measured_table`] states, and rise with the slider value.
//!
//! The counts must reach the floors in `tests/corpus_floors.json`, which only ever rise.
//!
//! `DALICA_FACES_RECORD=<dir>` writes what this run measured into `<dir>`: the reference files
//! (`reference/*.tsv`, for the corpus repository when the set is assembled or a pin moves) and
//! each subject's aligned crop (`crops/<person>/<nn>.ppm`, to check by eye that the subject is the
//! person named; never committed anywhere).
#![cfg(feature = "corpus")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dalica_faces::align::{Rgb, face_crop, template_transform};
use dalica_faces::calibration::{Confidence, STEPS};
use dalica_faces::cluster::{Groups, Member};
use dalica_faces::detect::{Detector, Face, iou};
use dalica_faces::embed::Embedder;
use dalica_faces::hash::sha256_file;
use dalica_faces::known;
use dalica_faces::manifest::ModelManifest;
use dalica_faces::matching::{Bars, Embedding, Gallery, Reference};
use dalica_faces::runtime::Model;
use zune_jpeg::JpegDecoder;
use zune_jpeg::zune_core::bytestream::ZCursor;
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;

/// The workspace root (this crate is `crates/faces`).
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

type Floors = BTreeMap<String, BTreeMap<String, u64>>;

fn floors() -> Floors {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("corpus_floors.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The portraits' set and the models' set.
const FACES: &str = "public-faces";
const MODELS: &str = "face-models";

/// A recorded reference vector and the one measured must agree this well (cosine): the same
/// pipeline on another machine differs only by float rounding.
const AGREEMENT: f32 = 0.999;
/// A subject is found where it was recorded when the boxes overlap this much.
const LOCATED_IOU: f32 = 0.9;
/// How far a committed calibration value may be from the one measured here (rounding to 0.001 and
/// float differences between machines).
const CALIBRATION_TOLERANCE: f32 = 0.002;

// ---------------------------------------------------------------------------------------------
// Manifests and verification

/// `(sha256, relative path)` entries of `xtask/corpus/<set>.sha256`.
fn manifest(set: &str) -> Vec<(String, String)> {
    let path = root().join("xtask").join("corpus").join(format!("{set}.sha256"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}: the {set} set has no manifest (not published yet?); see docs/development.md §11", path.display()));
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let (h, p) = l.split_once("  ").unwrap_or_else(|| panic!("{set}.sha256: bad line {l:?}"));
            (h.to_string(), p.to_string())
        })
        .collect()
}

/// Every file of the set present with its manifest's hash, or the test fails saying how to fix it.
fn verify(set: &str) -> (PathBuf, Vec<(String, String)>) {
    let dir = root().join("corpus").join(set);
    let entries = manifest(set);
    let bad: Vec<&str> = entries
        .iter()
        .filter(|(hash, rel)| sha256_file(&dir.join(rel)).ok().as_deref() != Some(hash.as_str()))
        .map(|(_, rel)| rel.as_str())
        .collect();
    assert!(
        bad.is_empty(),
        "corpus/{set}: {} of {} files missing or stale ({}); run `cargo xtask corpus --all`",
        bad.len(),
        entries.len(),
        bad.iter().take(5).copied().collect::<Vec<_>>().join(", ")
    );
    (dir, entries)
}

// ---------------------------------------------------------------------------------------------
// Pictures

struct Picture {
    data: Vec<u8>,
    width: usize,
    height: usize,
}

impl Picture {
    fn rgb(&self) -> Rgb<'_> {
        Rgb { data: &self.data, width: self.width, height: self.height }
    }
}

/// A portrait decoded to 8-bit RGB (a grey JPEG spread to three channels).
fn decode(path: &Path) -> Picture {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut probe = JpegDecoder::new(ZCursor::new(&bytes));
    probe.decode_headers().unwrap_or_else(|e| panic!("{}: {e:?}", path.display()));
    let grey = probe.input_colorspace() == Some(ColorSpace::Luma);
    let out = if grey { ColorSpace::Luma } else { ColorSpace::RGB };
    let mut d = JpegDecoder::new_with_options(ZCursor::new(&bytes), DecoderOptions::default().jpeg_set_out_colorspace(out));
    let pixels = d.decode().unwrap_or_else(|e| panic!("{}: {e:?}", path.display()));
    let (width, height) = d.dimensions().unwrap_or_else(|| panic!("{}: no size", path.display()));
    let data = if grey { pixels.iter().flat_map(|&v| [v, v, v]).collect() } else { pixels };
    assert_eq!(data.len(), width * height * 3, "{}", path.display());
    Picture { data, width, height }
}

/// One portrait of the set: `<person>/<nn>.jpg`.
struct Portrait {
    rel: String,
    person: String,
}

fn portraits(entries: &[(String, String)]) -> Vec<Portrait> {
    entries
        .iter()
        .filter(|(_, rel)| rel.ends_with(".jpg"))
        .map(|(_, rel)| {
            let (person, _) = rel.split_once('/').unwrap_or_else(|| panic!("{rel}: a portrait sits in its person's folder"));
            Portrait { rel: rel.clone(), person: person.to_string() }
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// References

/// `reference/<name>.tsv` of the set: `<portrait>\t<numbers, comma-separated>` per line, `#` lines
/// are comments.
fn read_reference(dir: &Path, name: &str) -> BTreeMap<String, Vec<f32>> {
    let path = dir.join("reference").join(format!("{name}.tsv"));
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("corpus/{FACES}: no reference/{name}.tsv: nothing agrees");
        return BTreeMap::new();
    };
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| {
            let (rel, numbers) = l.split_once('\t').unwrap_or_else(|| panic!("reference/{name}.tsv: bad line {l:?}"));
            let v = numbers.split(',').map(|n| n.trim().parse::<f32>().unwrap_or_else(|e| panic!("reference/{name}.tsv: {e}"))).collect();
            (rel.to_string(), v)
        })
        .collect()
}

fn reference_line(rel: &str, values: &[f32]) -> String {
    let numbers: Vec<String> = values.iter().map(|v| format!("{v:.6}")).collect();
    format!("{rel}\t{}\n", numbers.join(","))
}

/// The detector's reference numbers for a subject: the box, then the five landmarks.
fn face_numbers(f: &Face) -> Vec<f32> {
    let mut v = f.rect.to_vec();
    v.extend(f.landmarks.iter().flat_map(|(x, y)| [*x, *y]));
    v
}

fn record_dir() -> Option<PathBuf> {
    std::env::var_os("DALICA_FACES_RECORD").filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn write_record(name: &str, header: &str, body: &str) {
    let Some(dir) = record_dir() else { return };
    let path = dir.join("reference").join(format!("{name}.tsv"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, format!("{header}{body}")).unwrap();
    eprintln!("recorded {}", path.display());
}

/// A binary PPM of an RGB crop.
fn write_crop(rel: &str, crop: &[u8], side: usize) {
    let Some(dir) = record_dir() else { return };
    let path = dir.join("crops").join(rel.replace(".jpg", ".ppm"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut bytes = format!("P6\n{side} {side}\n255\n").into_bytes();
    bytes.extend_from_slice(crop);
    std::fs::write(path, bytes).unwrap();
}

// ---------------------------------------------------------------------------------------------
// Checks

/// The landmarks are in a face's order: eyes above the nose tip above the mouth corners, the
/// image's left before its right, all inside the box grown by half its size.
fn landmarks_plausible(f: &Face) -> bool {
    let [eye_l, eye_r, nose, mouth_l, mouth_r] = f.landmarks;
    let [x0, y0, x1, y1] = f.rect;
    let (pw, ph) = ((x1 - x0) * 0.5, (y1 - y0) * 0.5);
    let inside = f.landmarks.iter().all(|&(x, y)| x >= x0 - pw && x <= x1 + pw && y >= y0 - ph && y <= y1 + ph);
    let eyes = eye_l.1.max(eye_r.1);
    inside && eye_l.0 < eye_r.0 && mouth_l.0 < mouth_r.0 && eyes < nose.1 && nose.1 < mouth_l.1.min(mouth_r.1)
}

/// The calibration table measured on the set (`faces.md` §5): 85 is the model's recommended
/// `matchCosine` (measured on a larger set of named faces); 50 is the lowest cosine above every
/// pair of two different people here (the loosest setting still tells everyone in the set apart),
/// rounded up to 0.01; 95 is the median cosine of the pairs of one person (at the strictest setting
/// half of them still match), rounded to 0.01; the steps between are evenly spaced. Each anchor is
/// kept at least 0.01 per step from the next, so the table always rises.
fn measured_table(match_cosine: f32, impostor_max: f32, genuine_median: f32) -> [f32; 10] {
    let low = ((impostor_max * 100.0).ceil() / 100.0).min(match_cosine - 0.07);
    let high = ((genuine_median * 100.0).round() / 100.0).max(match_cosine + 0.02);
    let mut t = [0.0f32; 10];
    for (i, slot) in t.iter_mut().enumerate() {
        let v = if i <= 7 { low + (match_cosine - low) * i as f32 / 7.0 } else { match_cosine + (high - match_cosine) * (i - 7) as f32 / 2.0 };
        *slot = (v * 1000.0).round() / 1000.0;
    }
    t
}

fn median(mut v: Vec<f32>) -> f32 {
    v.sort_by(f32::total_cmp);
    match v.len() {
        0 => f32::NAN,
        n if n % 2 == 1 => v[n / 2],
        n => (v[n / 2 - 1] + v[n / 2]) / 2.0,
    }
}

/// The floors of one section: every floor met, every count with a floor (or listed as unfloored).
fn check_floors(section: &str, counts: &BTreeMap<String, u64>, unfloored: &[&str], failures: &mut Vec<String>) {
    let all = floors();
    let Some(f) = all.get(section) else {
        failures.push(format!("{section}: measured, but tests/corpus_floors.json has no floors for it: {counts:?}"));
        return;
    };
    for (name, &got) in counts {
        match f.get(name) {
            Some(&floor) if got < floor => failures.push(format!("{section}: {name} is {got}, below its floor {floor}")),
            Some(&floor) if got > floor => eprintln!("{section}: {name} is {got}, above its floor {floor}: raise the floor"),
            Some(_) => {}
            None if unfloored.contains(&name.as_str()) => {}
            None => failures.push(format!("{section}: {name} ({got}) has no floor")),
        }
    }
    for name in f.keys().filter(|k| !counts.contains_key(k.as_str())) {
        failures.push(format!("{section}: the floor {name} measures nothing"));
    }
}

/// The model file a known model is fetched as, in `corpus/face-models/`.
fn model_path(dir: &Path, m: &ModelManifest) -> PathBuf {
    let d = known::pinned(&m.id).unwrap_or_else(|| panic!("{}: no pinned address", m.id));
    dir.join(d.file_name)
}

/// One subject: its portrait, its face and its crop's place.
struct Subject {
    portrait: usize,
    face: Face,
}

#[test]
fn the_built_in_models_on_the_public_faces_corpus() {
    let (model_dir, model_entries) = verify(MODELS);
    // The pins of xtask are the ones known.rs records: same files, same hashes.
    let mut pinned: Vec<(String, String)> = known::all()
        .iter()
        .map(|m| {
            let d = known::pinned(&m.id).unwrap();
            (d.sha256, d.file_name)
        })
        .collect();
    pinned.sort_by(|a, b| a.1.cmp(&b.1));
    assert_eq!(model_entries, pinned, "xtask/corpus/{MODELS}.sha256 and dalica_faces::known pin different files");
    let (dir, entries) = verify(FACES);
    let portraits = portraits(&entries);
    let mut failures = Vec::new();

    // ---- detection and alignment
    let yunet = known::yunet();
    let detector = Detector::new(Model::open(&model_path(&model_dir, &yunet)).unwrap(), &yunet).unwrap();
    let opts = detector.options();
    let recorded = read_reference(&dir, &yunet.id);
    let mut pictures = Vec::new();
    let mut subjects: Vec<Subject> = Vec::new();
    let (mut faces_found, mut located, mut aligned) = (0u64, 0u64, 0u64);
    let mut record = String::new();
    for (i, p) in portraits.iter().enumerate() {
        let pic = decode(&dir.join(&p.rel));
        let found = detector.detect(&pic.rgb(), &opts).unwrap_or_else(|e| panic!("{}: {e}", p.rel));
        faces_found += found.len() as u64;
        let area = |f: &Face| (f.rect[2] - f.rect[0]) * (f.rect[3] - f.rect[1]);
        let Some(subject) = found.iter().copied().max_by(|a, b| area(a).total_cmp(&area(b))) else {
            eprintln!("{}: no face found", p.rel);
            pictures.push(pic);
            continue;
        };
        record.push_str(&reference_line(&p.rel, &face_numbers(&subject)));
        match recorded.get(&p.rel) {
            Some(r) if r.len() == 14 && iou(&[r[0], r[1], r[2], r[3]], &subject.rect) >= LOCATED_IOU => located += 1,
            _ => eprintln!("{}: the subject is not where it was recorded", p.rel),
        }
        let px = subject.landmarks.map(|(x, y)| (x * pic.width as f32 - 0.5, y * pic.height as f32 - 0.5));
        let crop = face_crop(&pic.rgb(), subject.rect, Some(&subject.landmarks), 112, 112);
        if landmarks_plausible(&subject) && template_transform(&px, 112, 112).is_some() && crop.is_some() {
            aligned += 1;
        } else {
            eprintln!("{}: the landmarks do not align", p.rel);
        }
        if let Some(c) = &crop {
            write_crop(&p.rel, c, 112);
        }
        subjects.push(Subject { portrait: i, face: subject });
        pictures.push(pic);
    }
    write_record(
        &yunet.id,
        "# The subject (largest face) of each portrait as dalica-faces' YuNet pipeline found it: x0,y0,x1,y1, then five landmarks (x,y), normalised.\n",
        &record,
    );
    let people: BTreeMap<&str, usize> = portraits.iter().fold(BTreeMap::new(), |mut m, p| {
        *m.entry(p.person.as_str()).or_default() += 1;
        m
    });
    let detection = BTreeMap::from([
        ("files".to_string(), portraits.len() as u64),
        ("people".to_string(), people.len() as u64),
        ("detected".to_string(), subjects.len() as u64),
        ("located".to_string(), located),
        ("aligned".to_string(), aligned),
        ("faces_found".to_string(), faces_found),
    ]);
    println!("{FACES}: {detection:?}");
    check_floors(FACES, &detection, &["faces_found"], &mut failures);

    // ---- each recogniser
    for m in [known::sface(), known::auraface()] {
        let embedder = Embedder::open(&model_path(&model_dir, &m), &m).unwrap_or_else(|e| panic!("{}: {e}", m.id));
        let reference = read_reference(&dir, &m.id);
        let mut vectors: Vec<(usize, Embedding)> = Vec::new();
        let mut agreeing = 0u64;
        let mut record = String::new();
        for s in &subjects {
            let p = &portraits[s.portrait];
            let pic = &pictures[s.portrait];
            let v = match embedder.embed_face(&pic.rgb(), s.face.rect, Some(&s.face.landmarks)) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{} ({}): {e}", p.rel, m.id);
                    continue;
                }
            };
            record.push_str(&reference_line(&p.rel, &v));
            let e = Embedding::new(&v).unwrap();
            match reference.get(&p.rel).and_then(|r| Embedding::new(r).ok()).and_then(|r| r.cosine(&e)) {
                Some(c) if c >= AGREEMENT => agreeing += 1,
                Some(c) => eprintln!("{} ({}): cosine {c:.5} with the recorded vector", p.rel, m.id),
                None => eprintln!("{} ({}): no recorded vector", p.rel, m.id),
            }
            vectors.push((s.portrait, e));
        }
        write_record(
            &m.id,
            &format!("# {} embeddings of each subject by dalica-faces' pipeline (YuNet, aligned to the 112 x 112 template), unit length.\n", m.id),
            &record,
        );
        let person = |i: usize| portraits[i].person.as_str();

        // pairs: one person (genuine) and two people (impostor)
        let (mut genuine, mut impostor) = (Vec::new(), Vec::new());
        for (a, (ia, ea)) in vectors.iter().enumerate() {
            for (ib, eb) in vectors.iter().skip(a + 1).map(|(i, e)| (*i, e)) {
                let c = ea.cosine(eb).unwrap();
                if person(*ia) == person(ib) { genuine.push(c) } else { impostor.push(c) }
            }
        }
        let impostor_max = impostor.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let genuine_median = median(genuine.clone());
        let match_cosine = m.thresholds.match_cosine.unwrap();
        let table = measured_table(match_cosine, impostor_max, genuine_median);
        println!(
            "{}: {} genuine pairs (median {genuine_median:.4}, lowest {:.4}), {} impostor pairs (highest {impostor_max:.4}); measured table {table:?}",
            m.id,
            genuine.len(),
            genuine.iter().copied().fold(f32::INFINITY, f32::min),
            impostor.len()
        );
        match m.calibration {
            Some(committed) => {
                if committed.iter().zip(&table).any(|(c, t)| (c - t).abs() > CALIBRATION_TOLERANCE) {
                    failures.push(format!("{}: the calibration in known.rs {committed:?} is not the measured {table:?}", m.id));
                }
                if committed.windows(2).any(|w| w[1] <= w[0]) {
                    failures.push(format!("{}: the calibration does not rise with the slider value: {committed:?}", m.id));
                }
                if (committed[7] - match_cosine).abs() > 1e-6 {
                    failures.push(format!("{}: the calibration at 85 is not the model's matchCosine {match_cosine}", m.id));
                }
            }
            None => failures.push(format!("{}: known.rs has no calibration table; the measured one is {table:?}", m.id)),
        }
        let calibration = m.calibration().unwrap();
        let at = |v: u8| calibration.threshold(Confidence::new(v).unwrap());

        // grouping at 85
        let threshold = at(85);
        let members = vectors.iter().map(|(i, e)| Member { face: *i, order: *i as u64, vec: e.pack() });
        let groups = Groups::build(threshold, members).groups();
        let mut people_whole = 0u64;
        for name in people.keys() {
            let holding: Vec<_> = groups.iter().filter(|g| g.faces.iter().any(|&f| person(f) == *name)).collect();
            let all_faces = vectors.iter().filter(|(i, _)| person(*i) == *name).count();
            if holding.len() == 1 && holding[0].faces.iter().filter(|&&f| person(f) == *name).count() == all_faces {
                people_whole += 1;
            } else {
                eprintln!("{}: {name} is split over {} groups at 85", m.id, holding.len());
            }
        }
        let faces_in_pure_groups: u64 =
            groups.iter().filter(|g| g.faces.iter().all(|&f| person(f) == person(g.faces[0]))).map(|g| g.faces.len() as u64).sum();

        // suggestions: each face hidden in turn, the others as everyone's references
        let exact = |f: &usize| vectors.iter().find(|(i, _)| i == f).map(|(_, e)| e.clone());
        let suggest_at = |value: u8| {
            let (mut made, mut right) = (0u64, 0u64);
            for (i, e) in &vectors {
                let mut gallery: Gallery<usize, &str> = Gallery::new();
                for name in people.keys() {
                    let refs: Vec<Reference<usize>> = vectors
                        .iter()
                        .filter(|(j, _)| j != i && person(*j) == *name)
                        .map(|(j, ej)| Reference { face: *j, taken: None, vec: ej.pack() })
                        .collect();
                    gallery.set_person(*name, refs);
                }
                if let Some(s) = gallery.suggest(i, &e.pack(), Bars::new(at(value)), &exact) {
                    made += 1;
                    right += u64::from(s.person == person(*i));
                }
            }
            (made, right)
        };
        let (suggested, suggested_right) = suggest_at(85);
        for value in STEPS {
            let (made, right) = suggest_at(value);
            println!("{}: confidence {value}: threshold {:.3}, {made} suggestions, {right} right", m.id, at(value));
        }
        let counts = BTreeMap::from([
            ("embedded".to_string(), vectors.len() as u64),
            ("agreeing".to_string(), agreeing),
            ("groups".to_string(), groups.len() as u64),
            ("people_whole".to_string(), people_whole),
            ("faces_in_pure_groups".to_string(), faces_in_pure_groups),
            ("suggested".to_string(), suggested),
            ("suggested_right".to_string(), suggested_right),
            ("precision_permille".to_string(), (suggested_right * 1000).checked_div(suggested).unwrap_or(0)),
        ]);
        println!("{}: {counts:?}", m.id);
        check_floors(&m.id, &counts, &["groups"], &mut failures);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
