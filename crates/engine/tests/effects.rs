//! Commands do what their specs declare, and the effect tabs' engine half.
//!
//! **Specs** (`roadmap.md` M10.35; `architecture.md` §3–§4): whatever
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
//!
//! **Effects** (`roadmap.md` M12.5; `editing.md` §8.1): `effect.list` is the catalogue in the
//! grids' order; a tab's tiles arrive in grid order at the size asked for, wait behind the edit
//! view's preview, are cancelled by another photo and remade after an edit, with no `fs` call on
//! the owner thread; `photo.effect.pick` answers a planted patch's colour on a turned photo.

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

// --- the effect tabs (M12.5) ---------------------------------------------------------------------

mod tabs {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use dalica_core::{PhotoId, SystemClock};
    use dalica_edit::catalog;
    use dalica_engine::cmd::effects::effect_json;
    use dalica_engine::watching::WatchMode;
    use dalica_engine::{CacheDir, CmdError, EffectTile, JobState, Options, Session, ThumbOptions};
    use dalica_fs::Expect;
    use serde_json::{Value, json};

    const WAIT: Duration = Duration::from_secs(60);
    const TILE: (u32, u32) = (78, 48);

    /// A library watching `Braga` with `photos` (name, bytes); ids in name order.
    fn library(photos: &[(&str, Vec<u8>)]) -> (tempfile::TempDir, PathBuf, Session, Vec<PhotoId>) {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Braga");
        dalica_fs::create_dir_all(&folder).unwrap();
        for (name, bytes) in photos {
            dalica_fs::replace(&folder.join(name), bytes, Expect::Absent).unwrap();
        }
        let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, workers: 1, ..ThumbOptions::default() } };
        let mut s = Session::open_with(&dir.path().join("library"), Arc::new(SystemClock), &options).unwrap();
        // A file removed by a test stays in the library.
        s.set_watching(WatchMode::Off);
        s.execute("folders.add", &json!({ "path": folder })).unwrap();
        s.wait_idle();
        let mut ids: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
        ids.sort();
        (dir, folder, s, ids.into_iter().map(|(_, id)| id).collect())
    }

    fn scenes(n: u64) -> Vec<(String, Vec<u8>)> {
        (0..n).map(|i| (format!("IMG_{i:04}.jpg"), dalica_scenes::jpeg(&dalica_scenes::Scene::new(i + 3, 480, 320), 85, None).unwrap())).collect()
    }

    fn owned(photos: &[(String, Vec<u8>)]) -> Vec<(&str, Vec<u8>)> {
        photos.iter().map(|(n, b)| (n.as_str(), b.clone())).collect()
    }

    /// Takes tiles until `n` have come or the wait is over.
    fn take(s: &mut Session, n: usize) -> Vec<EffectTile> {
        let until = Instant::now() + WAIT;
        let mut got = Vec::new();
        while got.len() < n && Instant::now() < until {
            assert!(s.wait_effect_tiles(WAIT));
            got.extend(s.take_effect_tiles());
        }
        got
    }

    fn names(tab: u8) -> Vec<&'static str> {
        catalog::effects(tab).map(|(op, _)| op.name).collect()
    }

    /// `roadmap.md` M12.5: `effect.list` gives the 27 effects in the three grids' order, equal to
    /// the catalogue, and one tab's alone; a tab that is not an effect tab is refused.
    #[test]
    fn effect_list_is_the_catalogue_in_the_grids_order() {
        let (_dir, _folder, mut s, _) = library(&[]);
        let all = s.execute("effect.list", &json!({})).unwrap();
        let expected: Vec<Value> = catalog::TABS.iter().flat_map(|t| catalog::effects(*t)).map(|(op, e)| effect_json(op, e)).collect();
        assert_eq!(all["effects"].as_array().unwrap().len(), 27);
        assert_eq!(all["effects"], json!(expected));
        for tab in catalog::TABS {
            let one = s.execute("effect.list", &json!({ "tab": tab })).unwrap();
            let listed = one["effects"].as_array().unwrap();
            assert!(listed.iter().all(|e| e["tab"] == json!(tab)) && listed.len() <= catalog::MAX_TILES);
            let places: Vec<u64> = listed.iter().map(|e| e["place"].as_u64().unwrap()).collect();
            assert_eq!(places, (0..listed.len() as u64).collect::<Vec<_>>(), "tab {tab} in grid order");
        }
        let sepia = &all["effects"][1];
        assert_eq!(
            (sepia["name"].clone(), sepia["oneClick"].clone(), sepia["defaultEntry"].clone()),
            (json!("sepia"), json!(true), json!("sepia()"))
        );
        for bad in [json!({ "tab": 2 }), json!({ "tab": "3" }), json!({ "tabs": 3 }), json!([3])] {
            assert!(matches!(s.execute("effect.list", &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
    }

    /// `roadmap.md` M12.5: tab 3's 12 tiles arrive in grid order at the size asked for, Sepia's
    /// has R ≥ G ≥ B everywhere (once its renderer is in), the owner thread makes no `fs` call,
    /// and another tab of the same photo needs no new decode.
    #[test]
    fn a_tabs_tiles_arrive_in_grid_order_at_the_size_asked_for() {
        let photos = scenes(1);
        let (_dir, _folder, mut s, ids) = library(&owned(&photos));
        let before = dalica_fs::guard::refused();
        let ui = dalica_fs::mark_ui_thread();
        s.request_effect_tiles(ids[0], 3, TILE).unwrap();
        let tiles = take(&mut s, 12);
        s.request_effect_tiles(ids[0], 4, (40, 40)).unwrap();
        let four = take(&mut s, 7);
        drop(ui);
        assert_eq!(dalica_fs::guard::refused(), before, "the owner thread made no fs call");

        assert_eq!(tiles.iter().map(|t| t.name).collect::<Vec<_>>(), names(3));
        assert_eq!(tiles.iter().map(|t| t.place).collect::<Vec<_>>(), (0..12).collect::<Vec<u8>>());
        for t in &tiles {
            assert_eq!((t.photo, t.tab, t.image.width, t.image.height), (ids[0], 3, TILE.0, TILE.1), "{}", t.name);
            assert_eq!(t.image.data.len(), (TILE.0 * TILE.1 * 4) as usize);
        }
        let tile = |name: &str| tiles.iter().find(|t| t.name == name).unwrap();
        if catalog::lookup("sepia").unwrap().renders() {
            for px in tile("sepia").image.data.chunks(4) {
                assert!(px[0] >= px[1] && px[1] >= px[2], "sepia gives R ≥ G ≥ B: {px:?}");
            }
        } else if !catalog::lookup("bw").unwrap().renders() {
            // Stubs change nothing: both tiles are the photo as it is.
            assert_eq!(tile("sepia").image.data, tile("bw").image.data);
        }
        assert_eq!(four.iter().map(|t| t.name).collect::<Vec<_>>(), names(4));
        assert!(four.iter().all(|t| (t.image.width, t.image.height) == (40, 40)));
        let stats = s.effect_tile_stats();
        assert_eq!((stats.decodes, stats.renders, stats.failures), (1, 19, 0), "{stats:?}");
        assert_eq!(s.effect_tiles_pending(), 0);
    }

    /// `roadmap.md` M12.5: a tile request waits behind the edit view's preview; an edit remakes
    /// the tiles on the new recipe; opening another photo cancels them.
    #[test]
    fn tiles_wait_behind_the_preview_are_remade_after_an_edit_and_cancelled_by_another_photo() {
        let photos = scenes(2);
        let (_dir, _folder, mut s, ids) = library(&owned(&photos));
        let (a, b) = (ids[0], ids[1]);
        s.open_preview(a, (300, 300)).unwrap();
        s.hold_preview(true);
        assert!(s.preview_busy() > 0);
        s.request_effect_tiles(a, 3, TILE).unwrap();
        for _ in 0..20 {
            s.poll();
            assert!(s.take_effect_tiles().is_empty(), "no tile while the preview is on its way");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!((s.effect_tile_stats().decodes, s.effect_tile_stats().renders), (0, 0));
        assert_eq!(s.effect_tiles_pending(), 12);
        s.hold_preview(false);
        let first = take(&mut s, 12);
        assert_eq!(first.len(), 12);
        assert!(s.preview().is_some(), "the preview came first");

        s.execute("photo.history.apply", &json!({ "id": a, "op": "rotate(90)" })).unwrap();
        let again = take(&mut s, 12);
        assert_eq!(again.len(), 12);
        assert!(again.iter().all(|t| t.recipe == ["rotate(90)"] && t.generation > first[0].generation));
        assert_eq!(s.effect_tile_stats().decodes, 1, "the small decode is kept");

        let cancelled = s.effect_tile_stats().cancelled;
        s.request_effect_tiles(a, 4, TILE).unwrap();
        assert!(s.effect_tiles_pending() > 0);
        s.open_preview(b, (300, 300)).unwrap();
        assert!(s.wait_effect_tiles(WAIT));
        assert!(s.take_effect_tiles().is_empty(), "nothing of the photo left is handed over");
        assert_eq!(s.effect_tile_stats().cancelled, cancelled + 7);
        assert_eq!(s.effect_tiles_pending(), 0);
    }

    /// Graceful failures of the tile requests: a tab that is not one, sizes of nothing or too
    /// large, an unknown photo; a photo whose file went missing makes no tile and waits for none.
    #[test]
    fn tile_requests_fail_gracefully() {
        let photos = scenes(1);
        let (_dir, folder, mut s, ids) = library(&owned(&photos));
        for tab in [0, 2, 6, 255] {
            assert!(matches!(s.request_effect_tiles(ids[0], tab, TILE), Err(CmdError::BadParams(_))), "{tab}");
        }
        for size in [(0, 48), (78, 0), (dalica_engine::effect_tiles::MAX_TILE + 1, 48)] {
            assert!(matches!(s.request_effect_tiles(ids[0], 3, size), Err(CmdError::BadParams(_))), "{size:?}");
        }
        assert!(matches!(s.request_effect_tiles(PhotoId::new(999_999), 3, TILE), Err(CmdError::NotFound(_))));
        std::fs::remove_file(folder.join(&photos[0].0)).unwrap();
        s.request_effect_tiles(ids[0], 3, TILE).unwrap();
        assert!(s.wait_effect_tiles(WAIT));
        assert!(s.take_effect_tiles().is_empty());
        assert_eq!(s.effect_tile_stats().failures, 1);
        s.close_effect_tiles();
        assert_eq!(s.effect_tiles_pending(), 0);
    }

    /// A 200 × 100 PNG: grey, with a patch of `PATCH` over x 0.1…0.3, y 0.2…0.6 (stored frame).
    fn planted() -> Vec<u8> {
        let (w, h) = (200u32, 100u32);
        let mut rgba = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let inside = (20..60).contains(&x) && (20..60).contains(&y);
                rgba.extend_from_slice(&if inside { [PATCH[0], PATCH[1], PATCH[2], 255] } else { [128, 128, 128, 255] });
            }
        }
        dalica_scenes::fingerprint::png_rgba(w, h, &rgba)
    }

    const PATCH: [u8; 3] = [220, 40, 90];

    fn pick(s: &mut Session, p: Value) -> Value {
        let v = s.execute("photo.effect.pick", &p).unwrap();
        assert_eq!(v["pending"], json!(true));
        let entry = s.wait_job(v["job"].as_u64().unwrap(), WAIT).unwrap();
        assert_eq!(entry.state, JobState::Done, "{entry:?}");
        entry.result.unwrap()
    }

    /// `roadmap.md` M12.5: `photo.effect.pick` answers a planted patch's colour on a turned photo
    /// (the point in the stored frame), with no `fs` call on the owner thread; its graceful
    /// failures.
    #[test]
    fn photo_effect_pick_answers_a_planted_patch_on_a_turned_photo() {
        let (_dir, _folder, mut s, ids) = library(&[("patch.png", planted())]);
        let id = ids[0];
        for op in ["rotate(90)", "flip_h()"] {
            s.execute("photo.history.apply", &json!({ "id": id, "op": op })).unwrap();
        }
        let before = dalica_fs::guard::refused();
        let ui = dalica_fs::mark_ui_thread();
        let on = pick(&mut s, json!({ "id": id, "x": 0.2, "y": 0.4 }));
        let off = pick(&mut s, json!({ "id": id, "x": 0.7, "y": 0.5 }));
        drop(ui);
        assert_eq!(dalica_fs::guard::refused(), before, "the owner thread made no fs call");
        assert_eq!(on, json!({ "id": id, "x": 0.2, "y": 0.4, "color": dalica_core::color::hex_u8(PATCH) }));
        assert_eq!(off["color"], json!("#808080"));
        // A crop that leaves the point out answers no colour.
        s.execute("photo.history.apply", &json!({ "id": id, "op": "crop(0.5,0,1,1)" })).unwrap();
        assert_eq!(pick(&mut s, json!({ "id": id, "x": 0.2, "y": 0.4 }))["color"], Value::Null);

        for bad in [
            json!({ "id": id, "x": 1.5, "y": 0.5 }),
            json!({ "id": id, "x": -0.1, "y": 0.5 }),
            json!({ "id": id, "x": 0.5 }),
            json!({ "id": id, "x": "a", "y": 0.5 }),
        ] {
            assert!(matches!(s.execute("photo.effect.pick", &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
        assert!(matches!(s.execute("photo.effect.pick", &json!({ "id": 999_999, "x": 0.5, "y": 0.5 })), Err(CmdError::NotFound(_))));
        let (stack, model) = (s.undo_stack().len(), s.library().model().clone());
        pick(&mut s, json!({ "id": id, "x": 0.5, "y": 0.5 }));
        assert_eq!((s.undo_stack().len(), s.library().model()), (stack, &model), "a pick changes nothing");
    }
}
