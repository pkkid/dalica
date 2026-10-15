//! Commands do what their specs declare (`roadmap.md` M10.35; `architecture.md` §3–§4): whatever
//! parameters it gets, a command whose spec says `Effect::Query` changes neither the model nor the
//! undo stack, and one whose spec says `History::None` adds no undo step. Each command's
//! parameters are mostly its own keys (read from its spec's `params`) with plausible values, so a
//! new command is checked without a change here.
//!
//! Each command runs on the same library: a folder of six photos and a subfolder of two (two
//! tagged, three in an album), with one step on the undo stack and one on the redo stack. A case
//! that left anything else (a `History::None` command may change the model: Rebuild, a setting) is
//! followed by the library put back as it was made. A task's gate runs a few cases per command, the
//! review gate (`DALICA_FULL_TESTS=1`) 64.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_core::SystemClock;
use dalica_engine::registry::{Effect, History};
use dalica_engine::watching::WatchMode;
use dalica_engine::{CacheDir, CmdError, Options, Session, ThumbOptions};
use dalica_library::Model;
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use serde_json::{Map, Value, json};

use crate::common::pick;

/// Photos in the library.
const PHOTOS: usize = 8;

/// What the undo stack shows: its depth, the redo stack's, and the labels on top.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Stack {
    undo: usize,
    redo: usize,
    undo_label: Option<&'static str>,
    redo_label: Option<&'static str>,
}

fn stack(s: &Session) -> Stack {
    let u = s.undo_stack();
    Stack { undo: u.len(), redo: u.redo_len(), undo_label: u.undo_label(), redo_label: u.redo_label() }
}

/// Any JSON value, a few levels deep.
fn any_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::from),
        (-2.0f64..2.0).prop_map(Value::from),
        "\\PC{0,12}".prop_map(Value::String),
    ];
    leaf.prop_recursive(2, 12, 3, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 0..3).prop_map(Value::Array),
            proptest::collection::btree_map("[a-z]{0,6}", inner, 0..3).prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

/// What the library holds, for plausible values: its folder paths (and one missing), its album,
/// a photo and its folder.
#[derive(Clone)]
struct Known {
    roots: Vec<PathBuf>,
    album: String,
    photo: u64,
    folder: u64,
}

/// A value for `key`, plausible for the keys commands commonly take, any JSON otherwise.
fn value_for(key: &str, known: &Known) -> BoxedStrategy<Value> {
    let (roots, album) = (known.roots.clone(), known.album.clone());
    let id = prop_oneof![4 => 0u64..12, 1 => any::<u64>()];
    let plausible = match key {
        "ids" => proptest::collection::vec(id, 0..5).prop_map(|v| json!(v)).boxed(),
        "id" | "folder" | "to" | "offset" | "limit" | "session" => id.prop_map(|n| json!(n)).boxed(),
        "path" | "dir" => prop_oneof![
            proptest::sample::select(roots.clone()).prop_map(|p| json!(p)),
            (proptest::sample::select(roots), "[a-z0-9]{1,8}").prop_map(|(p, s)| json!(p.join(s))),
        ]
        .boxed(),
        "album" => prop_oneof![Just(json!(album)), proptest::sample::select(vec!["starred", "recent", "a1", ""]).prop_map(|a| json!(a))].boxed(),
        "section" => proptest::sample::select(vec!["folder:1", "folder:2", "folder:999", "album:starred", "album:recent", "album:search", ""])
            .prop_map(|c| json!(c))
            .boxed(),
        "text" | "caption" | "name" | "tags" | "tag" | "prefix" | "place" | "description" => {
            prop_oneof![proptest::sample::select(vec!["beach", "tram", "Trip", ""]).prop_map(|t| json!(t)), "\\PC{0,12}".prop_map(|t| json!(t))]
                .boxed()
        }
        "command" => proptest::sample::select(dalica_engine::cmd::specs().iter().map(|s| s.id).collect::<Vec<_>>()).prop_map(|c| json!(c)).boxed(),
        _ => prop_oneof![any::<bool>().prop_map(Value::Bool), (-12i64..12).prop_map(|n| json!(n)), any_json()].boxed(),
    };
    prop_oneof![4 => plausible, 1 => any_json()].boxed()
}

/// The keys command `id` accepts, read from its spec's `params` (`{"ids":[PhotoId], "on":bool}:
/// …`): every quoted name followed by `:` or `?:` before the description.
fn own_keys(params: &'static str) -> Vec<&'static str> {
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

/// Parameters for a command with `params`: mostly its own keys, sometimes any JSON.
fn params(spec: &'static str, known: &Known) -> BoxedStrategy<Value> {
    let values: Vec<BoxedStrategy<Option<(String, Value)>>> = own_keys(spec)
        .into_iter()
        .map(|k| proptest::option::weighted(0.8, value_for(k, known)).prop_map(move |v| v.map(|v| (k.to_string(), v))).boxed())
        .collect();
    let object = values.prop_map(|kv| Value::Object(kv.into_iter().flatten().collect::<Map<String, Value>>()));
    prop_oneof![6 => object, 1 => any_json()].boxed()
}

/// The parameters every command runs with first, before the random ones: its own keys, each that
/// commands commonly take with a value that works (the library's photo, folder and album), so a
/// command that adds a step when it succeeds is caught in every run.
fn canonical(spec: &'static str, known: &Known) -> Value {
    let value = |key: &str| -> Option<Value> {
        Some(match key {
            "ids" => json!([known.photo]),
            "id" => json!(known.photo),
            "folder" => json!(known.folder),
            "path" | "dir" => json!(known.roots[0]),
            "album" => json!(known.album),
            "name" | "caption" | "text" | "description" | "place" => json!("Trip"),
            "tags" | "tag" => json!("beach"),
            "on" | "confirm" => json!(true),
            _ => return None,
        })
    };
    Value::Object(own_keys(spec).into_iter().filter_map(|k| value(k).map(|v| (k.to_string(), v))).collect())
}

fn open_session(lib: &Path) -> Session {
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, workers: 1, ..ThumbOptions::default() } };
    let mut s = Session::open_with(lib, Arc::new(SystemClock), &options).unwrap();
    s.set_watching(WatchMode::Off);
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

/// The library the commands run on, put back as it was made whenever a case changed it.
struct Bench {
    lib: tempfile::TempDir,
    photos: tempfile::TempDir,
    kept: tempfile::TempDir,
    session: Option<Session>,
    known: Known,
    /// The model and the stacks as the library was made.
    made: (Model, Stack),
}

impl Bench {
    fn new() -> Bench {
        let (lib, photos, kept) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let folder = photos.path().join("p");
        std::fs::create_dir_all(folder.join("q")).unwrap();
        for i in 0..PHOTOS as u64 {
            let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i, 64, 48), 70, None).unwrap();
            let dir = if i < 6 { folder.clone() } else { folder.join("q") };
            std::fs::write(dir.join(format!("{i}.jpg")), bytes).unwrap();
        }
        let mut s = open_session(lib.path());
        s.execute("folders.add", &json!({ "path": folder })).unwrap();
        s.wait_idle();
        let ids: Vec<u64> = s.library().photos().map(|p| p.id.get()).collect();
        s.execute("photo.tags.add", &json!({ "ids": ids[..2], "tags": "beach" })).unwrap();
        let album = s.execute("album.create", &json!({ "name": "Trip", "ids": ids[1..4] })).unwrap()["album"].as_str().unwrap().to_string();
        assert!(s.flush_metadata(std::time::Duration::from_secs(60)));
        s.flush().unwrap();
        drop(s);
        copy_tree(lib.path(), &kept.path().join("lib"));
        copy_tree(photos.path(), &kept.path().join("photos"));
        let roots = vec![folder.clone(), folder.join("q"), photos.path().join("missing")];
        let known = Known { roots, album, photo: ids[0], folder: 0 };
        let mut bench = Bench { lib, photos, kept, session: None, known, made: (Model::default(), stack_none()) };
        bench.reopen();
        let s = bench.session.as_ref().unwrap();
        bench.made = (s.library().model().clone(), stack(s));
        bench.known.folder = s.library().folder_by_path(&folder).unwrap().get();
        bench
    }

    /// Puts the library back as it was made and opens it, with a step on each stack: a star (to
    /// undo) and a caption (to redo).
    fn reopen(&mut self) {
        drop(self.session.take());
        put_back(&self.kept.path().join("lib"), self.lib.path());
        put_back(&self.kept.path().join("photos"), self.photos.path());
        let mut s = open_session(self.lib.path());
        let ids: Vec<u64> = s.library().photos().map(|p| p.id.get()).collect();
        s.execute("photo.star.set", &json!({ "ids": [ids[0]], "on": true })).unwrap();
        s.execute("photo.caption.set", &json!({ "ids": [ids[1]], "caption": "Kept for redo" })).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        s.wait_idle();
        self.session = Some(s);
    }

    /// The session, put back first when the last case left it otherwise than it was made.
    fn ready(&mut self) -> &mut Session {
        let whole = self.session.as_ref().is_some_and(|s| *s.library().model() == self.made.0 && stack(s) == self.made.1);
        if !whole {
            self.reopen();
        }
        self.session.as_mut().unwrap()
    }
}

fn stack_none() -> Stack {
    Stack { undo: 0, redo: 0, undo_label: None, redo_label: None }
}

/// Runs `id` with `p` on `s` and checks what its spec declares.
fn check(s: &mut Session, id: &str, effect: Effect, history: History, p: &Value) -> Result<(), TestCaseError> {
    let (model, before) = (s.library().model().clone(), stack(s));
    let r = s.execute(id, p);
    s.wait_idle();
    prop_assert!(!matches!(r, Err(CmdError::Internal(_))), "{id} panicked on {p}: {r:?}");
    let after = stack(s);
    if effect == Effect::Query {
        prop_assert!(*s.library().model() == model, "{id} ({p}) is a query but changed the model: {r:?}");
        prop_assert_eq!(&after, &before, "{} ({}) is a query but changed the undo stack", id, p);
    }
    if history == History::None {
        // Undo and Redo move a step from one stack to the other; nothing else may make one.
        let moved = matches!(id, "edit.undo" | "edit.redo");
        prop_assert!(after.undo + after.redo <= before.undo + before.redo, "{id} ({p}) added a step: {before:?} → {after:?}");
        prop_assert!(moved || after.undo <= before.undo, "{id} ({p}) added an undo step: {before:?} → {after:?}");
    }
    Ok(())
}

#[test]
fn queries_change_nothing_and_commands_without_history_add_no_step() {
    let _faults = crate::common::shared();
    let cases = pick(6, 64);
    let bench = Bench::new();
    assert_eq!(bench.made.1.undo, 1, "a step to undo");
    assert_eq!(bench.made.1.redo, 1, "a step to redo");
    let known = bench.known.clone();
    let bench = RefCell::new(bench);
    let specs: Vec<_> = dalica_engine::cmd::specs().into_iter().filter(|s| s.effect == Effect::Query || s.history == History::None).collect();
    let queries = specs.iter().filter(|s| s.effect == Effect::Query).count();
    for spec in &specs {
        let first = canonical(spec.params, &known);
        let result = check(bench.borrow_mut().ready(), spec.id, spec.effect, spec.history, &first);
        assert!(result.is_ok(), "{}: {result:?}", spec.id);
        // A runner stops after its case count, so each command gets its own.
        let mut runner = TestRunner::new(Config { cases, failure_persistence: None, ..Config::default() });
        let result = runner.run(&params(spec.params, &known), |p| check(bench.borrow_mut().ready(), spec.id, spec.effect, spec.history, &p));
        assert!(result.is_ok(), "{}: {result:?}", spec.id);
    }
    println!("effects: {} commands ({queries} queries, {} without history) x {cases} cases", specs.len(), specs.len() - queries);
}
