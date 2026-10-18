//! Downloads over the in-memory transport (`faces.md` §3.1, `security.md` §8.2): consent, a whole
//! download, a resume, wrong sizes and hashes, the cap, cancellation, the headers sent and hostile
//! specs. No network.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use dalica_fetch::spec::hex;
use dalica_fetch::{
    Consent, Downloaded, FetchError, FileSpec, HEADER_NAMES, MAX_CAP, MemoryTransport, Options, Progress, Served, SpecError, TransportError,
    USER_AGENT, download,
};
use proptest::prelude::*;
use sha2::{Digest, Sha256};

const URL: &str = "https://models.example.org/repo/raw/0123abcd/face.onnx";
const NAME: &str = "face.onnx";

/// Seeded bytes standing in for a model file.
fn file(len: usize, seed: u8) -> Vec<u8> {
    (0..len).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed) ^ ((i >> 8) as u8)).collect()
}

fn sha(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// A spec pinned to `bytes`' size and SHA-256.
fn pinned(bytes: &[u8]) -> FileSpec {
    FileSpec::new(URL, NAME, 1 << 20, Some(bytes.len() as u64), Some(&sha(bytes))).unwrap()
}

fn yes(spec: &FileSpec) -> Consent {
    Consent::acknowledged(spec, true).unwrap()
}

fn once() -> Options {
    Options { attempts: 1, ..Options::default() }
}

fn run(spec: &FileSpec, dir: &Path, transport: &MemoryTransport, opts: &Options) -> Result<Downloaded, FetchError> {
    download(spec, &yes(spec), dir, transport, opts, &AtomicBool::new(false), &mut |_| {})
}

/// The names in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

#[test]
fn nothing_is_requested_without_a_consent() {
    let bytes = file(5000, 1);
    let spec = pinned(&bytes);
    let other = FileSpec::new("https://models.example.org/other.onnx", "other.onnx", 1 << 20, None, None).unwrap();
    let transport = MemoryTransport::new();
    transport.serve(URL, Served::new(bytes));
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("models");

    // An unticked box makes no consent at all.
    assert_eq!(Consent::acknowledged(&spec, false), Err(FetchError::NoConsent));
    // A consent covers only the file it was given for.
    let theirs = yes(&other);
    assert!(!theirs.covers(&spec));
    let r = download(&spec, &theirs, &dir, &transport, &once(), &AtomicBool::new(false), &mut |_| {});
    assert_eq!(r, Err(FetchError::NoConsent));
    assert!(transport.requests().is_empty(), "no request without a consent");
    assert!(!dir.exists(), "nothing written without a consent");
}

#[test]
fn a_download_completes_and_verifies() {
    let bytes = file(50_000, 2);
    let spec = pinned(&bytes);
    let transport = MemoryTransport::new();
    transport.serve(URL, Served::new(bytes.clone()));
    let tmp = tempfile::tempdir().unwrap();
    let mut seen: Vec<Progress> = Vec::new();

    let done = download(&spec, &yes(&spec), tmp.path(), &transport, &once(), &AtomicBool::new(false), &mut |p| seen.push(*p)).unwrap();
    assert_eq!(done, Downloaded { path: tmp.path().join(NAME), size: 50_000, sha256: sha(&bytes), resumed_from: 0, fetched: true });
    assert_eq!(std::fs::read(tmp.path().join(NAME)).unwrap(), bytes);
    assert_eq!(names(tmp.path()), [NAME], "no partial file left behind");
    assert_eq!(seen.first(), Some(&Progress { done: 0, total: Some(50_000) }));
    assert_eq!(seen.last(), Some(&Progress { done: 50_000, total: Some(50_000) }));

    // The file in place matches its pins: kept, nothing requested again.
    let again = run(&spec, tmp.path(), &transport, &once()).unwrap();
    assert!(!again.fetched);
    assert_eq!(transport.requests().len(), 1);
}

#[test]
fn a_cut_download_resumes_from_its_part_and_verifies() {
    let bytes = file(50_000, 3);
    let spec = pinned(&bytes);
    let transport = MemoryTransport::new();
    transport.serve(URL, Served { cut_after: Some(12_288), ..Served::new(bytes.clone()) });
    let tmp = tempfile::tempdir().unwrap();

    let cut = run(&spec, tmp.path(), &transport, &once());
    assert!(matches!(cut, Err(FetchError::Transport(TransportError::Io(_)))), "{cut:?}");
    assert_eq!(names(tmp.path()), ["face.onnx.part"]);
    assert_eq!(std::fs::metadata(tmp.path().join("face.onnx.part")).unwrap().len(), 12_288);

    // The next run asks for the rest only, and the whole file verifies.
    transport.serve(URL, Served::new(bytes.clone()));
    let done = run(&spec, tmp.path(), &transport, &once()).unwrap();
    assert_eq!(done.resumed_from, 12_288);
    assert_eq!(done.sha256, sha(&bytes));
    let last = transport.requests().pop().unwrap();
    assert_eq!(last.from, 12_288);
    assert!(last.headers.contains(&("Range".to_string(), "bytes=12288-".to_string())));
    assert_eq!(std::fs::read(tmp.path().join(NAME)).unwrap(), bytes);
    assert_eq!(names(tmp.path()), [NAME]);

    // Within one call, every attempt resumes where the last one was cut.
    let tmp = tempfile::tempdir().unwrap();
    transport.serve(URL, Served { cut_after: Some(20_000), ..Served::new(bytes.clone()) });
    let done = run(&spec, tmp.path(), &transport, &Options { attempts: 3, ..Options::default() }).unwrap();
    assert_eq!(done.resumed_from, 40_000);
    assert_eq!(std::fs::read(tmp.path().join(NAME)).unwrap(), bytes);

    // A SHA-256 alone is pin enough to resume (the server's total is then only held to the cap).
    let tmp = tempfile::tempdir().unwrap();
    let hash_only = FileSpec::new(URL, NAME, 1 << 20, None, Some(&sha(&bytes))).unwrap();
    let done = run(&hash_only, tmp.path(), &transport, &Options { attempts: 3, ..Options::default() }).unwrap();
    assert_eq!((done.resumed_from, done.size), (40_000, 50_000));
}

#[test]
fn a_wrong_size_or_hash_deletes_the_file_and_says_so() {
    let bytes = file(30_000, 4);
    let spec = pinned(&bytes);
    let transport = MemoryTransport::new();
    let tmp = tempfile::tempdir().unwrap();

    // The same size, other bytes: the hash fails and the file is deleted.
    transport.serve(URL, Served::new(file(30_000, 5)));
    let e = run(&spec, tmp.path(), &transport, &once()).unwrap_err();
    assert!(matches!(&e, FetchError::WrongHash { expected, .. } if *expected == sha(&bytes)), "{e:?}");
    assert!(e.to_string().contains("deleted"), "{e}");
    assert!(names(tmp.path()).is_empty());

    // Another announced size: refused, the partial file from an earlier run deleted.
    dalica_fs::append(&tmp.path().join("face.onnx.part"), &bytes[..1000], 1 << 20).unwrap();
    transport.serve(URL, Served { ranges: false, ..Served::new(file(29_000, 4)) });
    let e = run(&spec, tmp.path(), &transport, &once()).unwrap_err();
    assert_eq!(e, FetchError::WrongSize { expected: 30_000, got: 29_000 });
    assert!(e.to_string().contains("deleted"), "{e}");
    assert!(names(tmp.path()).is_empty());

    // A file of that name that is not the pinned one is never replaced, and nothing is requested.
    std::fs::write(tmp.path().join(NAME), b"someone else's file").unwrap();
    let before = transport.requests().len();
    let e = run(&spec, tmp.path(), &transport, &once()).unwrap_err();
    assert_eq!(e, FetchError::Exists(tmp.path().join(NAME)));
    assert_eq!(transport.requests().len(), before);
    assert_eq!(std::fs::read(tmp.path().join(NAME)).unwrap(), b"someone else's file");
}

#[test]
fn a_reply_past_the_cap_stops_at_the_cap() {
    let spec = FileSpec::new(URL, NAME, 10_000, None, None).unwrap();
    let transport = MemoryTransport::new();
    let tmp = tempfile::tempdir().unwrap();

    // Unannounced: reading stops one byte past the cap and the partial file is deleted.
    transport.serve(URL, Served { announce: false, ..Served::new(file(1 << 20, 6)) });
    let e = run(&spec, tmp.path(), &transport, &once()).unwrap_err();
    assert_eq!(e, FetchError::TooLarge { limit: 10_000 });
    assert!(transport.bytes() <= 10_001, "read {} bytes past a cap of 10 000", transport.bytes());
    assert!(names(tmp.path()).is_empty());

    // Announced: refused before a byte of the body is read.
    let read = transport.bytes();
    transport.serve(URL, Served::new(file(1 << 20, 6)));
    assert_eq!(run(&spec, tmp.path(), &transport, &once()), Err(FetchError::TooLarge { limit: 10_000 }));
    assert_eq!(transport.bytes(), read);
    assert!(names(tmp.path()).is_empty());
}

#[test]
fn cancellation_stops_at_the_next_read() {
    let bytes = file(50_000, 7);
    let spec = pinned(&bytes);
    let transport = MemoryTransport::new();
    transport.serve(URL, Served::new(bytes));
    let tmp = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);

    let r = download(&spec, &yes(&spec), tmp.path(), &transport, &once(), &cancel, &mut |p| {
        if p.done > 0 {
            cancel.store(true, Ordering::Relaxed);
        }
    });
    assert_eq!(r, Err(FetchError::Cancelled));
    assert_eq!(transport.reads(), 1, "no read after the cancel");
    // The pinned partial file is kept to resume.
    assert_eq!(std::fs::metadata(tmp.path().join("face.onnx.part")).unwrap().len(), 4096);

    // Raised before the start: nothing is requested.
    let r = download(&spec, &yes(&spec), tmp.path(), &transport, &once(), &cancel, &mut |_| {});
    assert_eq!(r, Err(FetchError::Cancelled));
    assert_eq!(transport.requests().len(), 1);
}

#[test]
fn no_request_carries_an_identifying_header() {
    let bytes = file(20_000, 8);
    let spec = pinned(&bytes);
    let transport = MemoryTransport::new();
    transport.serve(URL, Served { cut_after: Some(8192), ..Served::new(bytes) });
    let tmp = tempfile::tempdir().unwrap();
    run(&spec, tmp.path(), &transport, &Options { attempts: 4, ..Options::default() }).unwrap();

    let requests = transport.requests();
    assert!(requests.len() >= 2, "a first request and a resume");
    let personal: Vec<String> =
        ["USER", "USERNAME", "HOSTNAME", "COMPUTERNAME"].iter().filter_map(|v| std::env::var(v).ok()).filter(|v| v.len() >= 3).collect();
    for r in &requests {
        assert_eq!(r.url, URL);
        for (name, value) in &r.headers {
            assert!(HEADER_NAMES.contains(&name.as_str()), "unexpected header {name}");
            assert!(!personal.iter().any(|p| value.contains(p.as_str())), "{name}: {value} names the user or machine");
        }
        assert!(r.headers.contains(&("User-Agent".to_string(), USER_AGENT.to_string())));
    }
    assert_eq!(USER_AGENT, concat!("Dalica/", env!("CARGO_PKG_VERSION")));
}

#[test]
fn hostile_specs_are_refused() {
    let sha = "a".repeat(64);
    let url = |u: &str| FileSpec::new(u, NAME, 1000, None, None);
    let name = |n: &str| FileSpec::new(URL, n, 1000, None, None);
    for bad in [
        "http://models.example.org/face.onnx",
        "ftp://x/y",
        "https://user:pw@example.org/f",
        "https://",
        "https://exa mple.org/",
        "https://h:0/",
        "https://h:99999/",
        "https://[::1/",
        "file:///etc/passwd",
        "",
    ] {
        assert!(matches!(url(bad), Err(SpecError::Url(_))), "{bad}");
    }
    assert!(url(&format!("https://example.org/{}", "a".repeat(10_000))).is_err());
    for bad in
        ["a/b", "a\\b", "..", ".", "", "C:x", "CON", "nul.txt", ".hidden", "face.onnx.part", "trail.", "a*b", "a\nb", "a\u{202e}b", &"n".repeat(251)]
    {
        assert_eq!(name(bad), Err(SpecError::Name(bad.to_string())), "{bad:?}");
    }
    for cap in [0, MAX_CAP + 1, u64::MAX] {
        assert_eq!(FileSpec::new(URL, NAME, cap, None, None), Err(SpecError::Cap(cap)));
    }
    assert_eq!(FileSpec::new(URL, NAME, 1000, Some(0), None), Err(SpecError::Size { size: 0, cap: 1000 }));
    assert_eq!(FileSpec::new(URL, NAME, 1000, Some(1001), None), Err(SpecError::Size { size: 1001, cap: 1000 }));
    for bad in ["", "abc", &"g".repeat(64), &"a".repeat(63), &"é".repeat(32)] {
        assert_eq!(FileSpec::new(URL, NAME, 1000, None, Some(bad)), Err(SpecError::Sha256), "{bad:?}");
    }
    // What is accepted: upper-case hex is read, the host lowered, the fragment dropped.
    let ok = FileSpec::new("HTTPS://Models.Example.org:8443/a?rev=1#x", NAME, MAX_CAP, Some(MAX_CAP), Some(&sha.to_uppercase())).unwrap();
    assert_eq!(ok.url().to_string(), "https://models.example.org:8443/a?rev=1");
    assert_eq!(ok.url().display(), "https://models.example.org:8443/a");
    assert_eq!(ok.sha256_hex(), Some(sha));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// No URL, name, cap or pin panics; whatever is accepted is an https address and a plain name.
    #[test]
    fn no_spec_panics(url in ".{0,64}", tail in "[a-zA-Z0-9:/@?#.\\[\\]%-]{0,40}", name in ".{0,40}", cap in any::<u64>(), size in any::<Option<u64>>(), sha in ".{0,70}") {
        for u in [url.as_str(), &format!("https://{tail}")] {
            if let Ok(spec) = FileSpec::new(u, &name, cap, size, Some(&sha)) {
                prop_assert!(spec.url().to_string().starts_with("https://"));
                prop_assert!(!spec.name().contains(['/', '\\']));
                prop_assert!(spec.cap() <= MAX_CAP && spec.size().is_none_or(|s| s <= spec.cap()));
            }
        }
    }
}
