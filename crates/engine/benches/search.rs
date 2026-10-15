//! Search of a 100 k-photo library (`organizing.md` §8.1, `development.md` §8, `roadmap.md`
//! M9.12). Run it with
//!
//! ```sh
//! cargo xtask bench search                                   # release: the 100 k library
//! cargo bench -p dalica-engine --bench search -- --json
//! taskset -c 0,1 target/release/deps/search-* --bench        # as on the budgets job's runner
//! ```
//!
//! The library is the synthetic 100 k one (`synthetic.rs`, 100 000 photos in 1 000 folders) with
//! what search reads added: a caption on every photo (three of [`WORDS`]), one to two of [`TAGS`],
//! search facts (a camera make on half, a GPS location on a quarter, one in twenty a video), and
//! [`ALBUMS`] albums of [`ALBUM_PHOTOS`] photos each spread over the library. It is written as an
//! index snapshot and opened as a session on a virtual clock, as `tests/search.rs` does.
//!
//! Rows (times are p50 / p95 / max by nearest rank; the budgets are judged on the median by
//! `cargo xtask budgets`, which runs this bench on the budgets job):
//!
//! - `index build`: `SearchIndex::from_library` of the whole library (the extract and the index),
//!   what the search worker does with a fresh copy. Budget [`INDEX_BUDGET_MS`].
//! - `one word`, `three words and a filter`, `a -word`: `SearchIndex::search` of [`ONE_WORD`],
//!   [`THREE_WORDS`] with Starred on, and [`NOT_WORD`] (a lone negation: nearly every photo
//!   matches, the largest result), in the scope the worker builds (`search::scope_of` with the
//!   session's `search_tree_options`). Budgets [`WORD_BUDGET_MS`], [`FILTER_BUDGET_MS`],
//!   [`NOT_BUDGET_MS`].
//! - `search.set to applied`: from `search.set {text: ONE_WORD}` to its result applied by
//!   `Session::poll`, the debounce excluded (the virtual clock is moved past it at once): the
//!   owner's send, the worker's scope, query and suggestions, the reply, and the result applied.
//!   `search.clear` (not timed) runs between samples so each `search.set` changes the query.
//!   Budget [`APPLIED_BUDGET_MS`].
//! - `first search` (not budgeted): the same for the session's first search, which also starts the
//!   worker, copies the library to it and builds its index.
//!
//! Every query budget is 1.5 × its first measurement and never above [`QUERY_CAP_MS`].
//!
//! Files go under `--dir` (default: this target directory's temporary folder) and are removed
//! afterwards. Without `--bench` (under `cargo test --benches`) it runs a smoke pass on a small
//! library (`--photos` 2 000, two samples each).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dalica_core::{AlbumId, PhotoId, Timestamp, VirtualClock};
use dalica_engine::search::{DEBOUNCE, scope_of};
use dalica_engine::synthetic::{self, SEED, Spec};
use dalica_engine::{CacheDir, Options, Session, ThumbOptions};
use dalica_library::search::{Filters, Query, SearchIndex};
use dalica_library::{AlbumInfo, Facts, Library, Op};
use serde_json::{Value, json};

/// The budgets (`development.md` §8), judged on the median: 1.5 × the first measurement, taken in
/// release on two cores (`taskset -c 0,1`, as the budgets job's runner) on 2026-10-14, a 24-core
/// Linux workstation under other agents' builds (load average 3.5): the index built in 417 ms, one
/// word 1.12 ms, three words and Starred 0.38 ms, a `-word` 3.39 ms, `search.set` to applied
/// 14.9 ms; rounded up to 5 ms for the build and to 0.1 ms for a query, and for a query never above
/// [`QUERY_CAP_MS`]. `xtask/src/accept.rs` holds the same numbers.
const INDEX_BUDGET_MS: f64 = 630.0;
const WORD_BUDGET_MS: f64 = 1.7;
const FILTER_BUDGET_MS: f64 = 0.6;
const NOT_BUDGET_MS: f64 = 5.1;
const APPLIED_BUDGET_MS: f64 = 22.3;
/// No query's budget is above this (`roadmap.md` M9.12).
const QUERY_CAP_MS: f64 = 100.0;
const _: () = assert!(WORD_BUDGET_MS <= QUERY_CAP_MS && FILTER_BUDGET_MS <= QUERY_CAP_MS && NOT_BUDGET_MS <= QUERY_CAP_MS);
const _: () = assert!(APPLIED_BUDGET_MS <= QUERY_CAP_MS);

/// The queries.
const ONE_WORD: &str = "sunset";
const THREE_WORDS: &str = "beach sunset 2021";
const NOT_WORD: &str = "-sunset";

/// Caption words (our own list); each caption takes three, so a word is in about one photo in
/// eight.
const WORDS: [&str; 24] = [
    "sunset",
    "beach",
    "tram",
    "market",
    "castle",
    "bridge",
    "garden",
    "street",
    "picnic",
    "birthday",
    "snow",
    "boat",
    "train",
    "mountain",
    "museum",
    "concert",
    "dinner",
    "kite",
    "lighthouse",
    "waterfall",
    "festival",
    "parade",
    "harvest",
    "cathedral",
];
/// Tags (our own list).
const TAGS: [&str; 30] = [
    "family",
    "holiday",
    "work",
    "friends",
    "nature",
    "city",
    "food",
    "pets",
    "sport",
    "travel",
    "school",
    "wedding",
    "garden",
    "night",
    "portrait",
    "landscape",
    "architecture",
    "street",
    "macro",
    "winter",
    "summer",
    "spring",
    "autumn",
    "music",
    "art",
    "cars",
    "birds",
    "flowers",
    "sea",
    "hiking",
];
const MAKES: [&str; 4] = ["Canon", "Nikon", "Sony", "Fujifilm"];
const ALBUMS: u64 = 100;
const ALBUM_PHOTOS: u64 = 300;
/// 2026-06-10T00:00:00 UTC.
const NOW_MS: i64 = 1_781_049_600_000;
const WAIT: Duration = Duration::from_secs(600);

struct Args {
    photos: usize,
    samples: usize,
    builds: usize,
    dir: PathBuf,
    json: bool,
}

fn args() -> Result<Args, String> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let full = all.iter().any(|a| a == "--bench");
    let mut a = Args {
        photos: if full { Spec::PHOTOS_100K.photos } else { 2_000 },
        samples: if full { 50 } else { 2 },
        builds: if full { 5 } else { 1 },
        dir: PathBuf::from(env!("CARGO_TARGET_TMPDIR")),
        json: false,
    };
    let mut it = all.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{arg} needs a value"));
        let number = |v: String| v.parse::<usize>().map_err(|e| format!("{arg}: {e}"));
        match arg.as_str() {
            "--photos" => a.photos = number(value()?)?,
            "--samples" => a.samples = number(value()?)?,
            "--builds" => a.builds = number(value()?)?,
            "--dir" => a.dir = PathBuf::from(value()?),
            "--json" => a.json = true,
            _ => {}
        }
    }
    if !(1_000..=synthetic::MAX_PHOTOS).contains(&a.photos) {
        return Err(format!("--photos takes 1000 … {}", synthetic::MAX_PHOTOS));
    }
    if !(1..=1_000).contains(&a.samples) || !(1..=100).contains(&a.builds) {
        return Err("--samples takes 1 … 1000, --builds 1 … 100".into());
    }
    Ok(a)
}

/// The nearest-rank `p`th percentile of `sorted`.
fn rank(sorted: &[f64], p: usize) -> f64 {
    sorted.get((sorted.len() * p).div_ceil(100).saturating_sub(1)).copied().unwrap_or(0.0)
}

fn row(name: &str, mut times: Vec<f64>, budget: Option<f64>, matches: Option<usize>) -> Value {
    times.sort_by(f64::total_cmp);
    let mut r = json!({
        "name": name,
        "p50_ms": rank(&times, 50),
        "p95_ms": rank(&times, 95),
        "max_ms": times.last().copied().unwrap_or(0.0),
        "samples": times.len(),
    });
    if let Some(b) = budget {
        r["budget_ms"] = json!(b);
    }
    if let Some(m) = matches {
        r["matches"] = json!(m);
    }
    r
}

/// A SplitMix64-style finaliser: the pseudo-random choices of photo `i`.
fn mix(i: u64) -> u64 {
    let mut z = i.wrapping_add(SEED).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

fn pick<'a>(list: &[&'a str], bits: u64) -> &'a str {
    list[(bits % list.len() as u64) as usize]
}

/// The synthetic library of `photos` photos with captions, tags, facts and albums.
fn library(photos_dir: &std::path::Path, photos: usize) -> Library {
    let spec = Spec::new(photos, (photos / 100).max(1), SEED);
    let mut lib = synthetic::library_at(photos_dir, &spec).unwrap();
    let ids: Vec<PhotoId> = lib.photos().map(|p| p.id).collect();
    let mut captions = Vec::with_capacity(ids.len());
    let mut tagged: Vec<Vec<PhotoId>> = vec![Vec::new(); TAGS.len()];
    let mut facts = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        let h = mix(i as u64);
        captions.push((*id, format!("{} and {} by the {}", pick(&WORDS, h), pick(&WORDS, h >> 8), pick(&WORDS, h >> 16))));
        tagged[(h >> 24) as usize % TAGS.len()].push(*id);
        if (h >> 32).is_multiple_of(3) {
            tagged[(h >> 36) as usize % TAGS.len()].push(*id);
        }
        let make = if (h >> 44).is_multiple_of(2) { pick(&MAKES, h >> 45).to_string() } else { String::new() };
        let f = Facts { make, model: String::new(), video: (h >> 48).is_multiple_of(20), geotagged: (h >> 54).is_multiple_of(4) };
        if f != Facts::default() {
            facts.push((*id, f));
        }
    }
    let mut ops = vec![Op::SetCaptions { captions }, Op::SetFacts { facts }];
    for (tag, ids) in TAGS.iter().zip(tagged) {
        let mut ids = ids;
        ids.dedup();
        ops.push(Op::AddTags { ids, tags: vec![(*tag).to_string()] });
    }
    lib.apply(Op::Batch { ops }).unwrap();
    let n = ids.len() as u64;
    for a in 0..ALBUMS {
        let album = AlbumId::parse(&format!("a{:012x}", a + 1)).unwrap();
        lib.apply(Op::CreateAlbums { albums: vec![(album.clone(), AlbumInfo::named(format!("Album {a} {}", pick(&WORDS, a))))] }).unwrap();
        let mut members: Vec<PhotoId> = (0..ALBUM_PHOTOS.min(n)).map(|k| ids[((a * 7919 + k * 331) % n) as usize]).collect();
        members.sort();
        members.dedup();
        let op = lib.album_add_op(&album, &members, None).unwrap();
        lib.apply(op).unwrap();
    }
    lib
}

/// Opens `lib` as a session in `dir` on a virtual clock (`tests/search.rs` → `fixture`).
fn open(dir: &std::path::Path, photos_dir: &std::path::Path, lib: &Library) -> (Session, Arc<VirtualClock>) {
    let lib_dir = dir.join("library");
    dalica_fs::create_dir_all(&lib_dir).unwrap();
    dalica_store::snapshot::write(&lib_dir.join(dalica_store::snapshot::FILE), lib, 0).unwrap();
    let roots = json!({ "version": 1, "folders": [{ "path": photos_dir }] });
    dalica_fs::replace(&lib_dir.join(dalica_engine::session::FOLDERS_FILE), &serde_json::to_vec(&roots).unwrap(), dalica_fs::Expect::Absent).unwrap();
    let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(NOW_MS)));
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, ..ThumbOptions::default() } };
    (Session::open_with(&lib_dir, clock.clone(), &options).unwrap(), clock)
}

/// `search.set {text}`, the debounce passed on the virtual clock, then `poll` until the result is
/// applied: the wall time from the command to the result applied, and the matches.
fn set_to_applied(s: &mut Session, clock: &VirtualClock, text: &str) -> (f64, usize) {
    let t = Instant::now();
    s.execute("search.set", &json!({ "text": text })).unwrap();
    clock.advance(DEBOUNCE);
    loop {
        s.poll();
        let st = s.search_state();
        if !st.pending()
            && let Some(r) = st.result.as_ref().filter(|r| r.generation == st.generation)
        {
            return (t.elapsed().as_secs_f64() * 1000.0, r.count());
        }
        assert!(t.elapsed() < WAIT, "no result for {text:?} in {WAIT:?}");
        std::thread::yield_now();
    }
}

fn main() {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("search: {e}");
            std::process::exit(2);
        }
    };
    let root = a.dir.join(format!("search-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    dalica_fs::create_dir_all(&root).unwrap();
    let photos_dir = synthetic::photos_dir(&root);
    let made = Instant::now();
    let lib = library(&photos_dir, a.photos);
    let (mut s, clock) = open(&root, &photos_dir, &lib);
    drop(lib);
    let photos = s.library().photos().count();
    let folders = s.library().folders().count();
    eprintln!("a library of {photos} photos in {folders} folders, {ALBUMS} albums, made and opened in {:.1} s", made.elapsed().as_secs_f64());

    // The session's first search: the worker starts, takes a copy and builds its index.
    let (first, _) = set_to_applied(&mut s, &clock, ONE_WORD);
    s.execute("search.clear", &json!({})).unwrap();

    let mut builds = Vec::new();
    let mut index = SearchIndex::default();
    for _ in 0..a.builds {
        let t = Instant::now();
        index = SearchIndex::from_library(s.library());
        builds.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    assert_eq!(index.len(), photos);

    let scope = scope_of(s.library(), s.roots(), &s.search_tree_options());
    let starred = Filters { starred: true, ..Filters::default() };
    let queries = [
        ("one word", Query::text(ONE_WORD), WORD_BUDGET_MS),
        ("three words and a filter", Query { text: THREE_WORDS.into(), filters: starred, age: None }, FILTER_BUDGET_MS),
        ("a -word", Query::text(NOT_WORD), NOT_BUDGET_MS),
    ];
    let mut rows = vec![row("index build", builds, Some(INDEX_BUDGET_MS), None)];
    for (name, query, budget) in &queries {
        let matches = index.search(query, &scope).count();
        let mut times = Vec::with_capacity(a.samples);
        for _ in 0..a.samples {
            let t = Instant::now();
            let r = index.search(query, &scope);
            times.push(t.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(r.count(), matches);
        }
        rows.push(row(name, times, Some(*budget), Some(matches)));
    }
    assert!(rows.iter().filter_map(|r| r["matches"].as_u64()).all(|m| m > 0), "every query matches something: {rows:?}");

    let mut applied = Vec::with_capacity(a.samples);
    let mut matches = 0;
    for _ in 0..a.samples {
        let (ms, n) = set_to_applied(&mut s, &clock, ONE_WORD);
        applied.push(ms);
        matches = n;
        s.execute("search.clear", &json!({})).unwrap();
    }
    assert_eq!(Some(matches as u64), rows.get(1).and_then(|r| r["matches"].as_u64()), "the worker answers as the index does");
    rows.push(row("search.set to applied", applied, Some(APPLIED_BUDGET_MS), Some(matches)));
    rows.push(row("first search", vec![first], None, None));
    drop(s);
    let _ = std::fs::remove_dir_all(&root);

    let mode = if cfg!(debug_assertions) { "debug" } else { "release" };
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    if a.json {
        println!(
            "{}",
            json!({ "bench": "search", "mode": mode, "threads": threads, "photos": photos, "folders": folders, "albums": ALBUMS,
                    "query_cap_ms": QUERY_CAP_MS, "rows": rows })
        );
    } else {
        for r in &rows {
            println!("{}", serde_json::to_string_pretty(r).unwrap_or_default());
        }
    }
}
