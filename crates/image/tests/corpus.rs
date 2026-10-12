//! The corpus tests (`docs/development.md` §11), compiled only with the `corpus` feature: real
//! files from the pinned test corpora in `corpus/<set>/`, fetched by `cargo xtask corpus --all`
//! and run by `cargo xtask test-corpus` (`--release --features corpus,heif`).
//!
//! Each set is checked against its manifest `xtask/corpus/<set>.sha256` first: a missing or stale
//! file is a failure ("run `cargo xtask corpus --all`"), never a skip. Then every file is sniffed,
//! its header read the way scanning reads it, decoded in full and as a 256 px thumbnail, and its
//! embedded thumbnail decoded; the counts must reach the committed floors in
//! `tests/corpus_floors.json`, which only ever rise. A set without a manifest is not published yet
//! (our own `dalica-corpus` sets until the lead pins them): it is reported as skipped, and having
//! a floor for it is a failure. Corpus files are never committed or quoted; a file that newly
//! fails gets a synthetic reproduction in the ordinary tests.
#![cfg(feature = "corpus")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dalica_image::{Error, Format, Limits, RawKind, decode, decode_thumbnail, read_header, read_header_file, sniff};

/// The workspace root (this crate is `crates/image`).
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

type Floors = BTreeMap<String, BTreeMap<String, u64>>;

fn floors() -> Floors {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("corpus_floors.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The sets this crate tests.
const SETS: &[&str] = &["pngsuite", "raw", "photos", "heif"];

/// The format a file's extension promises (`None`: not a picture, e.g. a README or licence).
fn by_extension(rel: &str) -> Option<Format> {
    let ext = rel.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())?;
    Some(match ext.as_str() {
        "jpg" | "jpeg" => Format::Jpeg,
        "png" => Format::Png,
        "tif" | "tiff" => Format::Tiff,
        "webp" => Format::WebP,
        "gif" => Format::Gif,
        "bmp" => Format::Bmp,
        "heic" | "heif" => Format::Heif,
        "avif" => Format::Avif,
        "jxl" => Format::JpegXl,
        "psd" | "psb" => Format::Psd,
        "cr3" => Format::Raw(RawKind::Cr3),
        "raf" => Format::Raw(RawKind::Raf),
        "crw" => Format::Raw(RawKind::Crw),
        "mrw" => Format::Raw(RawKind::Mrw),
        "x3f" => Format::Raw(RawKind::X3f),
        "dng" | "cr2" | "nef" | "nrw" | "arw" | "pef" | "orf" | "rw2" | "srw" => Format::Raw(RawKind::TiffBased),
        _ => return None,
    })
}

// ---------------------------------------------------------------------------------------------
// Manifests and verification

/// `(sha256, relative path)` entries, or `None` when the set has no manifest (not published yet).
fn manifest(set: &str) -> Option<Vec<(String, String)>> {
    let path = root().join("xtask").join("corpus").join(format!("{set}.sha256"));
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => panic!("{}: {e}", path.display()),
    };
    let entries = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let (h, p) = l.split_once("  ").unwrap_or_else(|| panic!("{}: bad line {l:?}", path.display()));
            assert!(h.len() == 64 && !p.split('/').any(|c| c.is_empty() || c == ".."), "{}: bad line {l:?}", path.display());
            (h.to_string(), p.to_string())
        })
        .collect();
    Some(entries)
}

/// SHA-256 (FIPS 180-4) of `data`, lower-case hex: the manifests' check, without a dependency.
fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be,
        0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa,
        0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85,
        0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f,
        0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut tail = data.as_chunks::<64>().1.to_vec();
    tail.push(0x80);
    while tail.len() % 64 != 56 {
        tail.push(0);
    }
    tail.extend_from_slice(&(data.len() as u64).wrapping_mul(8).to_be_bytes());
    for block in data.as_chunks::<64>().0.iter().chain(tail.as_chunks::<64>().0) {
        let mut w = [0u32; 64];
        for (i, word) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let t1 = hh
                .wrapping_add(e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25))
                .wrapping_add((e & f) ^ (!e & g))
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let t2 = (a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22)).wrapping_add((a & b) ^ (a & c) ^ (b & c));
            (hh, g, f, e, d, c, b, a) = (g, f, e, d.wrapping_add(t1), c, b, a, t1.wrapping_add(t2));
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|v| format!("{v:08x}")).collect()
}

#[test]
fn sha256_known_answers() {
    assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    let long = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
    assert_eq!(sha256_hex(long), "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
    assert_eq!(sha256_hex(&[b'a'; 1000]), "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3");
}

// ---------------------------------------------------------------------------------------------
// Measuring a set

/// What one file did.
struct Outcome {
    rel: String,
    format: String,
    header: Result<(), String>,
    decoded: Result<(u32, u32), Error>,
    thumbnail: bool,
    embedded: Option<bool>,
}

fn measure_file(path: &Path, rel: &str, bytes: &[u8], limits: &Limits) -> (Outcome, bool) {
    let sniffed = sniff(bytes);
    let header = read_header_file(path, limits).map(|_| ()).map_err(|e| e.reason());
    let decoded = decode(bytes, limits).map(|p| (p.width, p.height));
    let thumbnail = decode_thumbnail(bytes, 256, limits).is_ok_and(|t| t.width.max(t.height) <= 256 && t.width > 0);
    let embedded = read_header(bytes, limits).ok().and_then(|h| h.thumbnail(bytes).map(|t| decode(t, limits).is_ok()));
    let format = sniffed.map_or("unknown".to_string(), |f| format!("{f:?}"));
    let as_expected = sniffed.is_some() && sniffed == by_extension(rel);
    (Outcome { rel: rel.to_string(), format, header, decoded, thumbnail, embedded }, as_expected)
}

/// Verifies the set against its manifest and measures it; `Err` is why it cannot be measured.
fn measure(set: &str) -> Option<Result<BTreeMap<String, u64>, String>> {
    let entries = manifest(set)?;
    let dir = root().join("corpus").join(set);
    let limits = Limits::default();
    let (mut missing, mut stale) = (Vec::new(), Vec::new());
    let mut m: BTreeMap<String, u64> = BTreeMap::new();
    let mut outcomes = Vec::new();
    for (hash, rel) in &entries {
        let path = dir.join(rel);
        let Ok(bytes) = std::fs::read(&path) else {
            missing.push(rel.clone());
            continue;
        };
        if sha256_hex(&bytes) != *hash {
            stale.push(rel.clone());
            continue;
        }
        if by_extension(rel).is_none() {
            continue; // READMEs and licences travel with the set
        }
        let (outcome, as_expected) = measure_file(&path, rel, &bytes, &limits);
        // PngSuite's x*.png files are corrupt on purpose: refusing them is the right answer.
        let corrupt = set == "pngsuite" && rel.starts_with('x');
        let mut count = |k: &str, yes: bool| *m.entry(k.to_string()).or_default() += u64::from(yes);
        count("files", true);
        count("sniffed", as_expected);
        if corrupt {
            count("corrupt", true);
            count("corrupt_refused", outcome.decoded.is_err());
        } else {
            count("headers", outcome.header.is_ok());
            count("decoded", outcome.decoded.is_ok());
            count("thumbnails", outcome.thumbnail);
            count("embedded_thumbnails", outcome.embedded == Some(true));
        }
        count("caught_panics", matches!(outcome.decoded, Err(Error::Panic(_))));
        outcomes.push(outcome);
    }
    if !missing.is_empty() || !stale.is_empty() {
        return Some(Err(format!(
            "corpus/{set}: {} of {} files missing ({}), {} stale ({}) against xtask/corpus/{set}.sha256: run `cargo xtask corpus --all`",
            missing.len(),
            entries.len(),
            some(&missing),
            stale.len(),
            some(&stale)
        )));
    }
    println!("\ncorpus/{set}: {} files", outcomes.len());
    for o in &outcomes {
        let decoded = match &o.decoded {
            Ok((w, h)) => format!("{w} × {h}"),
            Err(e) => format!("refused: {} ({e})", e.reason()),
        };
        let header = o.header.as_ref().map_or_else(|e| format!("header: {e}"), |_| "header ok".into());
        let embedded = match o.embedded {
            Some(true) => "embedded ok",
            Some(false) => "embedded broken",
            None => "no embedded",
        };
        println!("  {:<32} {:<16} {header:<14} {decoded:<40} thumb {} · {embedded}", o.rel, o.format, if o.thumbnail { "ok" } else { "no" });
    }
    Some(Ok(m))
}

/// The first few names of a list, for messages.
fn some(names: &[String]) -> String {
    let shown: Vec<&str> = names.iter().take(8).map(String::as_str).collect();
    if names.len() > shown.len() { format!("{}, …", shown.join(", ")) } else { shown.join(", ") }
}

/// Measures `set` and holds it to its floors; `Ok(None)` when it is not published yet.
fn check(set: &str) -> Result<Option<BTreeMap<String, u64>>, String> {
    let floors = floors();
    let Some(measured) = measure(set) else {
        if floors.contains_key(set) {
            return Err(format!("{set} has floors in corpus_floors.json but no manifest xtask/corpus/{set}.sha256"));
        }
        println!("corpus/{set}: not published yet (no xtask/corpus/{set}.sha256); SKIPPED, not passed, and it has no floors");
        return Ok(None);
    };
    let measured = measured?;
    println!("corpus/{set} measured: {measured:?}");
    let floor = floors.get(set).ok_or_else(|| format!("{set} is measured but has no floors in corpus_floors.json: add {measured:?}"))?;
    let mut problems = Vec::new();
    for (key, min) in floor {
        match measured.get(key) {
            None => problems.push(format!("{set}.{key}: a floor for something this test does not measure")),
            Some(got) if got < min => problems.push(format!("{set}.{key}: {got} is below its floor {min}")),
            Some(got) if got > min => println!("corpus/{set}.{key}: {got} is above its floor {min}; raise the floor in this PR"),
            Some(_) => {}
        }
    }
    for key in measured.keys().filter(|k| !floor.contains_key(*k) && *k != "caught_panics") {
        problems.push(format!("{set}.{key} has no floor (add it at the measured count)"));
    }
    if problems.is_empty() { Ok(Some(measured)) } else { Err(problems.join("\n")) }
}

fn run(set: &str) {
    if let Err(e) = check(set) {
        panic!("{e}");
    }
}

#[test]
fn floors_name_known_sets_only() {
    for set in floors().keys() {
        assert!(SETS.contains(&set.as_str()), "corpus_floors.json names an unknown set {set:?}");
    }
}

#[test]
fn pngsuite() {
    run("pngsuite");
}

#[test]
fn raw() {
    run("raw");
}

#[test]
fn photos() {
    run("photos");
}

#[test]
fn heif() {
    // Without the feature HEIC files are refused by design; measuring them then would read as a
    // regression, and skipping them would be a silent pass.
    if manifest("heif").is_some() && !cfg!(feature = "heif") {
        panic!("the heif set needs `--features heif` (cargo xtask test-corpus turns it on)");
    }
    run("heif");
}
