//! `panic_hunt` (`AGENTS.md` §3, `development.md` §10): every registered command, fed random and
//! hostile parameters — alone and in random sequences — on one live session. No command may panic
//! (an `internal` error), and the session must stay consistent.
//!
//! Slow, so it is ignored by default: `cargo test -p dalica-engine --test panic_hunt -- --ignored`.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_core::SystemClock;
use dalica_engine::{CmdError, Session};
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use serde_json::{Map, Value, json};

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

/// A value for one of the keys commands know, often plausible, sometimes not.
fn value_for(key: &'static str, roots: Vec<PathBuf>) -> BoxedStrategy<Value> {
    let ids = proptest::collection::vec(prop_oneof![4 => 0u64..12, 1 => any::<u64>()], 0..6).prop_map(|v| json!(v));
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
        "rect" => proptest::collection::vec(prop_oneof![4 => 0.0f64..1.0, 1 => any::<f64>()], 0..6).prop_map(|v| json!(v)).boxed(),
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
        "folder" | "offset" | "limit" => prop_oneof![0u64..20, Just(10_000), Just(10_001), any::<u64>()].prop_map(|n| json!(n)).boxed(),
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

/// The keys command `id` accepts (so most generated parameters get past parsing).
fn own_keys(id: &str) -> Vec<&'static str> {
    match id {
        "photo.star.toggle" | "photo.inspect" => vec!["ids"],
        "photo.star.set" => vec!["ids", "on"],
        "library.query" => vec!["folder", "starred", "offset", "limit"],
        "folders.add" => vec!["path"],
        "app.setShortcut" => vec!["command", "context", "key", "replace"],
        "app.resetShortcuts" => vec!["command"],
        "photo.history.apply" => vec!["id", "op"],
        "photo.crop.apply" => vec!["id", "rect", "ratio"],
        "photo.straighten.apply" => vec!["id", "degrees"],
        "crop.ratios.add" => vec!["name", "width", "height"],
        "crop.ratios.remove" => vec!["name"],
        "photo.crop.suggest" => vec!["id", "ratio"],
        "photo.history.tuning" => vec!["id", "session", "fill", "highlights", "shadows", "temp", "neutral"],
        "photo.tuning.auto" => vec!["id", "what"],
        "photo.tuning.pick" => vec!["id", "x", "y"],
        "photo.history.undo" | "photo.history.redo" | "photo.history.state" | "edit.undo" | "edit.redo" => vec!["id"],
        "photo.save" => vec!["ids", "confirm", "dontAsk"],
        "photo.undoSave" => vec!["ids"],
        "photo.revert" => vec!["ids", "confirm", "action"],
        "photo.saveCopy" => vec!["ids"],
        "photo.saveTo" => vec!["id", "path", "format"],
        _ => vec![],
    }
}

fn object(keys: Vec<&'static str>, roots: Vec<PathBuf>) -> BoxedStrategy<Value> {
    let values: Vec<BoxedStrategy<Option<(String, Value)>>> = keys
        .into_iter()
        .map(|k| proptest::option::weighted(0.85, value_for(k, roots.clone())).prop_map(move |v| v.map(|v| (k.to_string(), v))).boxed())
        .collect();
    values.prop_map(|kv| Value::Object(kv.into_iter().flatten().collect::<Map<String, Value>>())).boxed()
}

/// Parameters for command `id`: mostly its own keys with plausible values, sometimes any known
/// keys, sometimes arbitrary JSON.
fn params(id: &str, roots: Vec<PathBuf>) -> BoxedStrategy<Value> {
    let any_keys = {
        let roots = roots.clone();
        proptest::collection::vec(proptest::sample::select(KEYS.to_vec()), 0..5).prop_flat_map(move |keys| object(keys, roots.clone()))
    };
    prop_oneof![6 => object(own_keys(id), roots), 2 => any_keys, 1 => any_json()].boxed()
}

/// A session over a library holding a folder of 6 photos.
fn setup() -> (tempfile::TempDir, tempfile::TempDir, Session, Vec<PathBuf>) {
    let (lib_dir, photos) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = photos.path().join("p");
    std::fs::create_dir_all(&folder).unwrap();
    for i in 0..6 {
        let exif = dalica_scenes::Exif { orientation: 1 + (i % 8) as u8, ..dalica_scenes::Exif::default() };
        std::fs::write(folder.join(format!("{i}.jpg")), dalica_scenes::jpeg(&dalica_scenes::Scene::new(i, 64, 48), 70, Some(&exif)).unwrap())
            .unwrap();
    }
    let mut s = Session::open(lib_dir.path(), Arc::new(SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let roots = vec![folder, photos.path().join("missing")];
    (lib_dir, photos, s, roots)
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

fn run(s: &RefCell<Session>, id: &str, p: &Value) -> Result<(), TestCaseError> {
    let mut s = s.borrow_mut();
    let r = s.execute(id, p);
    s.wait_idle();
    prop_assert!(!matches!(r, Err(CmdError::Internal(_))), "{id} panicked on {p}: {r:?}");
    check(&s)
}

#[test]
#[ignore = "slow: cargo test -p dalica-engine --test panic_hunt -- --ignored"]
fn every_command_survives_random_parameters() {
    let (_lib, _photos, session, roots) = setup();
    let session = RefCell::new(session);
    let ids: Vec<&'static str> = dalica_engine::cmd::specs().iter().map(|s| s.id).collect();
    for id in &ids {
        // A runner stops after its case count, so each command gets its own.
        let mut runner = TestRunner::new(Config { cases: 300, failure_persistence: None, ..Config::default() });
        let result = runner.run(&params(id, roots.clone()), |p| run(&session, id, &p));
        assert!(result.is_ok(), "{id}: {result:?}");
    }
    println!("panic_hunt: {} commands x 300 cases", ids.len());
}

#[test]
#[ignore = "slow: cargo test -p dalica-engine --test panic_hunt -- --ignored"]
fn random_command_sequences_survive() {
    let (_lib, _photos, session, roots) = setup();
    let session = RefCell::new(session);
    let ids: Vec<&'static str> = dalica_engine::cmd::specs().iter().map(|s| s.id).collect();
    let step = proptest::sample::select(ids).prop_flat_map(move |id| (Just(id), params(id, roots.clone())));
    let mut runner = TestRunner::new(Config { cases: 64, failure_persistence: None, ..Config::default() });
    let result = runner.run(&proptest::collection::vec(step, 1..40), |steps| {
        for (id, p) in steps {
            run(&session, id, &p)?;
        }
        let mut s = session.borrow_mut();
        let flushed = s.flush();
        prop_assert!(!matches!(flushed, Err(CmdError::Internal(_))));
        Ok(())
    });
    assert!(result.is_ok(), "{result:?}");
}

/// Keeps `Path` in use for readers of this file: roots are absolute temp paths.
#[allow(dead_code)]
fn is_temp(p: &Path) -> bool {
    p.starts_with(std::env::temp_dir())
}
