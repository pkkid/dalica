//! `panic_hunt` (`AGENTS.md` §3, `development.md` §10): every registered command, fed random and
//! hostile parameters — alone and in random sequences — on one live session. No command may panic
//! (an `internal` error), and the session must stay consistent. Each command's parameters are
//! mostly its own keys, read from its spec's `params`, so a new command's own keys are fuzzed
//! without a change here.
//!
//! A task's gate runs a few cases per command and a few short sequences; the review gate
//! (`DALICA_FULL_TESTS=1`) 300 cases per command and 64 sequences of up to 40 commands:
//! `DALICA_FULL_TESTS=1 cargo test -p dalica-engine --test all panic_hunt::`.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_core::SystemClock;
use dalica_engine::{CacheDir, CmdError, Options, Session, ThumbOptions};
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use serde_json::{Map, Value, json};

use crate::common::pick;

/// Any JSON value, a few levels deep.
fn any_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::from),
        any::<u64>().prop_map(Value::from),
        any::<f64>().prop_filter("finite", |f| f.is_finite()).prop_map(Value::from),
        "\\PC{0,16}".prop_map(Value::String),
    ];
    leaf.prop_recursive(3, 24, 4, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            proptest::collection::btree_map("\\PC{0,8}", inner, 0..4).prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

/// What the hunt's library holds, for plausible values: its folder paths (and one missing) and its
/// album's id.
#[derive(Clone)]
struct Known {
    roots: Vec<PathBuf>,
    albums: Vec<String>,
}

/// A value for one of the keys commands know, often plausible, sometimes not.
fn value_for(key: &'static str, known: Known) -> BoxedStrategy<Value> {
    let ids = proptest::collection::vec(prop_oneof![4 => 0u64..12, 1 => any::<u64>()], 0..6).prop_map(|v| json!(v));
    let roots = known.roots.clone();
    let path = prop_oneof![
        proptest::sample::select(roots.clone()).prop_map(|p| json!(p)),
        (proptest::sample::select(roots), "[a-z0-9]{1,8}").prop_map(|(p, s)| json!(p.join(s))),
        // Relative paths (refused): never starting with `/`, so a fuzzed path cannot reach `/dev`.
        "[a-z.][a-z./]{0,11}".prop_map(|s| json!(s)),
    ];
    let commands: Vec<&'static str> = dalica_engine::cmd::specs().iter().map(|s| s.id).collect();
    let plausible = match key {
        "ids" => ids.boxed(),
        "id" => prop_oneof![4 => 0u64..12, 1 => any::<u64>()].prop_map(|n| json!(n)).boxed(),
        "op" => prop_oneof![
            proptest::sample::select(vec![
                "rotate(90)",
                "rotate(270)",
                "rotate(45)",
                "flip_h()",
                "flip_v()",
                "crop(0.1,0.1,0.9,0.9)",
                "tuning(0.2,0,0,0,none)",
                "autofix()",
                "sepia()",
                "future_op(1)",
                "legacy:redeye(1)",
                "",
                "sepia(",
            ])
            .prop_map(|o| json!(o)),
            "\\PC{0,24}".prop_map(|o| json!(o))
        ]
        .boxed(),
        "on" | "starred" | "replace" | "confirm" | "dontAsk" => any::<bool>().prop_map(Value::Bool).boxed(),
        "action" => proptest::sample::select(vec!["revert", "undoSave", "", "delete"]).prop_map(|a| json!(a)).boxed(),
        "path" => path.boxed(),
        "rect" => prop_oneof![
            proptest::sample::select(vec![[0.1, 0.1, 0.8, 0.8], [0.0, 0.0, 1.0, 1.0], [0.5, 0.25, 0.5, 0.5]]).prop_map(|r| json!(r)),
            proptest::collection::vec(prop_oneof![4 => 0.0f64..1.0, 1 => any::<f64>()], 0..6).prop_map(|v| json!(v)),
        ]
        .boxed(),
        "degrees" | "width" | "height" | "ratio" => prop_oneof![4 => -12.0f64..12.0, 1 => any::<f64>()].prop_map(|n| json!(n)).boxed(),
        "name" => "\\PC{0,12}".prop_map(|c| json!(c)).boxed(),
        "session" => prop_oneof![0u64..4, any::<u64>()].prop_map(|n| json!(n)).boxed(),
        "fill" | "highlights" | "shadows" | "temp" | "x" | "y" => prop_oneof![4 => -0.2f64..1.2, 1 => any::<f64>()].prop_map(|n| json!(n)).boxed(),
        "neutral" => prop_oneof![
            proptest::sample::select(vec!["#808080", "#A0B0c0", "none", "#12345", "red", ""]).prop_map(|c| json!(c)),
            Just(Value::Null),
            proptest::collection::vec(prop_oneof![4 => 0.0f64..1.0, 1 => any::<f64>()], 0..5).prop_map(|v| json!(v)),
        ]
        .boxed(),
        "format" => proptest::sample::select(vec!["jpeg", "png", "tiff", ""]).prop_map(|f| json!(f)).boxed(),
        "what" => proptest::sample::select(vec!["lighting", "color", "colour", ""]).prop_map(|c| json!(c)).boxed(),
        "folder" | "offset" | "limit" | "before" | "after" => {
            prop_oneof![4 => 0u64..4, 1 => 0u64..20, 1 => Just(10_000), 1 => Just(10_001), 1 => any::<u64>()].prop_map(|n| json!(n)).boxed()
        }
        "to" => prop_oneof![0u64..6, any::<u64>()].prop_map(|n| json!(n)).boxed(),
        "dir" => path.boxed(),
        "album" => {
            let names = [known.albums.clone(), ["a1", "a91c0e", "starred", "recent", "", "A1234"].map(String::from).to_vec()].concat();
            prop_oneof![2 => proptest::sample::select(known.albums.clone()), 1 => proptest::sample::select(names)].prop_map(|a| json!(a)).boxed()
        }
        "section" => proptest::sample::select(vec![
            "folder:1",
            "folder:2",
            "folder:999",
            "folder:x",
            "album:starred",
            "album:recent",
            "album:search",
            "album:a1",
            "",
        ])
        .prop_map(|c| json!(c))
        .boxed(),
        "collisions" => proptest::sample::select(vec!["rename", "skip", "replace", ""]).prop_map(|c| json!(c)).boxed(),
        "by" => proptest::sample::select(vec!["date", "name", "size", "colour"]).prop_map(|c| json!(c)).boxed(),
        "mode" => proptest::sample::select(vec!["always", "once", "remove", "sometimes"]).prop_map(|c| json!(c)).boxed(),
        "quality" => prop_oneof![
            proptest::sample::select(vec!["automatic", "normal", "maximum", "minimum", "custom", "best"]).prop_map(|c| json!(c)),
            (0u64..120).prop_map(|n| json!(n)),
        ]
        .boxed(),
        "size" => prop_oneof![(0u64..5000).prop_map(|n| json!(n)), Just(json!("original"))].boxed(),
        "n" | "age" | "shift" => prop_oneof![4 => -12i64..12, 1 => any::<i64>()].prop_map(|n| json!(n)).boxed(),
        "set" | "date" => proptest::sample::select(vec!["2026-06-04T10:00:00", "2026-06-04", "2026-02-30", "oldest", "yesterday", ""])
            .prop_map(|d| json!(d))
            .boxed(),
        "all" | "refresh" | "reverse" | "useNewest" | "useOldest" | "useRecent" | "addDate" | "addSize" | "resize" | "numbers" | "removeLocation" => {
            any::<bool>().prop_map(Value::Bool).boxed()
        }
        "tags" | "tag" | "from" => {
            prop_oneof![proptest::sample::select(vec!["beach", "tram", "Beach", ""]).prop_map(|t| json!(t)), "\\PC{0,16}".prop_map(|c| json!(c))]
                .boxed()
        }
        "text" | "caption" | "prefix" | "place" | "description" | "folderName" | "watermark" => "\\PC{0,16}".prop_map(|c| json!(c)).boxed(),
        "command" => prop_oneof![proptest::sample::select(commands).prop_map(|c| json!(c)), "\\PC{0,12}".prop_map(|c| json!(c))].boxed(),
        "context" => prop_oneof![
            proptest::sample::select(vec!["global", "library", "editView", "slideshow", "textInput", "kitchen", ""]).prop_map(|c| json!(c)),
            any_json()
        ]
        .boxed(),
        "key" => prop_oneof![
            proptest::sample::select(vec!["Ctrl+8", "F4", "Ctrl+Z", "Ctrl+Shift+9", "Alt+Enter", "Hyper+X", "", "Ctrl++"]).prop_map(|k| json!(k)),
            Just(Value::Null),
            any_json()
        ]
        .boxed(),
        _ => any_json().boxed(),
    };
    prop_oneof![4 => plausible, 1 => any_json()].boxed()
}

const KEYS: [&str; 14] = ["ids", "id", "op", "on", "path", "folder", "starred", "offset", "limit", "command", "context", "key", "replace", "zzz"];

/// The keys command `id` accepts, read from its spec's `params` (`{"ids":[PhotoId], "on":bool}:
/// …`): every quoted name followed by `:` or `?:` before the description, so most generated
/// parameters get past parsing and reach the command's `run`.
fn own_keys(id: &str) -> Vec<&'static str> {
    let Some(spec) = dalica_engine::cmd::specs().into_iter().find(|s| s.id == id) else { return Vec::new() };
    let params: &'static str = spec.params;
    let shape = params.find("}: ").map_or(params, |end| &params[..end]);
    let mut keys: Vec<&'static str> = Vec::new();
    for (i, _) in shape.match_indices('"') {
        let rest = &shape[i + 1..];
        let Some(end) = rest.find('"') else { break };
        let (key, after) = (&rest[..end], &rest[end + 1..]);
        let named = !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric());
        if named && (after.starts_with(':') || after.starts_with("?:")) && !keys.contains(&key) {
            keys.push(key);
        }
    }
    keys
}

fn object(keys: Vec<&'static str>, known: Known) -> BoxedStrategy<Value> {
    let values: Vec<BoxedStrategy<Option<(String, Value)>>> = keys
        .into_iter()
        .map(|k| proptest::option::weighted(0.85, value_for(k, known.clone())).prop_map(move |v| v.map(|v| (k.to_string(), v))).boxed())
        .collect();
    values.prop_map(|kv| Value::Object(kv.into_iter().flatten().collect::<Map<String, Value>>())).boxed()
}

/// Parameters for command `id`: mostly its own keys with plausible values, sometimes any known
/// keys, sometimes arbitrary JSON.
fn params(id: &str, known: Known) -> BoxedStrategy<Value> {
    let any_keys = {
        let known = known.clone();
        proptest::collection::vec(proptest::sample::select(KEYS.to_vec()), 0..5).prop_flat_map(move |keys| object(keys, known.clone()))
    };
    prop_oneof![6 => object(own_keys(id), known), 2 => any_keys, 1 => any_json()].boxed()
}

/// Photos in the hunt's library.
const PHOTOS: usize = 8;

/// A session over a library holding a folder of six photos and a subfolder of two (two tagged
/// "beach", three in an album, quick tags 3 and 4 assigned), kept whole between commands: a
/// copy of the library and its folder made once is put back (at the same paths) whenever a command
/// left fewer photos or other watched folders.
struct Hunt {
    lib: tempfile::TempDir,
    photos: tempfile::TempDir,
    kept: tempfile::TempDir,
    session: RefCell<Option<Session>>,
    known: Known,
}

fn options() -> Options {
    Options { thumbs: ThumbOptions { cache: CacheDir::Temp, workers: 1, ..ThumbOptions::default() } }
}

fn open_session(lib: &Path) -> Session {
    let mut s = Session::open_with(lib, Arc::new(SystemClock), &options()).unwrap();
    s.wait_idle();
    s
}

/// Copies the folder `from` into `to` (made), everything in it included.
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        if e.file_type().unwrap().is_dir() {
            copy_tree(&e.path(), &to.join(e.file_name()));
        } else {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}

/// Empties `dir` and fills it with a copy of `from`.
fn put_back(from: &Path, dir: &Path) {
    for e in std::fs::read_dir(dir).unwrap() {
        let path = e.unwrap().path();
        if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) }.unwrap();
    }
    copy_tree(from, dir);
}

impl Hunt {
    fn new() -> Hunt {
        let (lib, photos, kept) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let folder = photos.path().join("p");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::create_dir_all(folder.join("q")).unwrap();
        for i in 0..PHOTOS as u64 {
            let exif = dalica_scenes::Exif { orientation: 1 + (i % 8) as u8, ..dalica_scenes::Exif::default() };
            let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i, 64, 48), 70, Some(&exif)).unwrap();
            let dir = if i < 6 { folder.clone() } else { folder.join("q") };
            std::fs::write(dir.join(format!("{i}.jpg")), bytes).unwrap();
        }
        let mut s = open_session(lib.path());
        s.execute("folders.add", &json!({ "path": folder })).unwrap();
        s.wait_idle();
        let ids: Vec<u64> = s.library().photos().map(|p| p.id.get()).collect();
        s.execute("photo.tags.add", &json!({ "ids": ids[..2], "tags": "beach" })).unwrap();
        s.execute("tags.quick.set", &json!({ "slots": [null, null, "tram", "beach"] })).unwrap();
        let album = s.execute("album.create", &json!({ "name": "Trip", "ids": ids[1..4] })).unwrap()["album"].as_str().unwrap().to_string();
        s.flush().unwrap();
        drop(s);
        copy_tree(lib.path(), &kept.path().join("lib"));
        copy_tree(photos.path(), &kept.path().join("photos"));
        let session = RefCell::new(Some(open_session(lib.path())));
        let known = Known { roots: vec![folder.clone(), folder.join("q"), photos.path().join("missing")], albums: vec![album] };
        Hunt { lib, photos, kept, session, known }
    }

    /// Puts the library and its photos back as they were made if a command took photos or watched
    /// folders away.
    fn heal(&self) {
        let mut session = self.session.borrow_mut();
        let whole = session.as_ref().is_some_and(|s| s.library().photos().count() == PHOTOS && s.roots() == &self.known.roots[..1]);
        if whole {
            return;
        }
        drop(session.take());
        put_back(&self.kept.path().join("lib"), self.lib.path());
        put_back(&self.kept.path().join("photos"), self.photos.path());
        *session = Some(open_session(self.lib.path()));
    }

    /// Runs `id` with `p`: never a panic, and the session still answers sensibly.
    fn run(&self, id: &str, p: &Value) -> Result<(), TestCaseError> {
        let mut session = self.session.borrow_mut();
        let s = session.as_mut().unwrap();
        let r = s.execute(id, p);
        s.wait_idle();
        prop_assert!(!matches!(r, Err(CmdError::Internal(_))), "{id} panicked on {p}: {r:?}");
        check(s)
    }
}

/// The session still answers sensibly after whatever happened.
fn check(s: &Session) -> Result<(), TestCaseError> {
    let lib = s.library();
    for p in lib.photos() {
        prop_assert!(lib.folder(p.folder).is_some(), "photo {} has no folder", p.id);
        prop_assert_eq!(lib.photo_by_name(p.folder, &p.name).map(|q| q.id), Some(p.id));
    }
    Ok(())
}

#[test]
fn every_command_survives_random_parameters() {
    let _faults = crate::common::shared();
    let cases = pick(8, 300);
    let hunt = Hunt::new();
    let ids: Vec<&'static str> = dalica_engine::cmd::specs().iter().map(|s| s.id).collect();
    for id in &ids {
        // A runner stops after its case count, so each command gets its own.
        let mut runner = TestRunner::new(Config { cases, failure_persistence: None, ..Config::default() });
        let result = runner.run(&params(id, hunt.known.clone()), |p| {
            hunt.heal();
            hunt.run(id, &p)
        });
        assert!(result.is_ok(), "{id}: {result:?}");
    }
    println!("panic_hunt: {} commands x {cases} cases", ids.len());
}

#[test]
fn random_command_sequences_survive() {
    let _faults = crate::common::shared();
    let (sequences, longest) = pick((4, 10), (64, 40));
    let hunt = Hunt::new();
    let ids: Vec<&'static str> = dalica_engine::cmd::specs().iter().map(|s| s.id).collect();
    let known = hunt.known.clone();
    let step = proptest::sample::select(ids).prop_flat_map(move |id| (Just(id), params(id, known.clone())));
    let mut runner = TestRunner::new(Config { cases: sequences, failure_persistence: None, ..Config::default() });
    let result = runner.run(&proptest::collection::vec(step, 1..longest), |steps| {
        hunt.heal();
        for (id, p) in steps {
            hunt.run(id, &p)?;
        }
        let mut session = hunt.session.borrow_mut();
        let flushed = session.as_mut().unwrap().flush();
        prop_assert!(!matches!(flushed, Err(CmdError::Internal(_))));
        Ok(())
    });
    assert!(result.is_ok(), "{result:?}");
}
