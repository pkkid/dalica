//! Test helpers: a session on a temporary library over a folder of `scenes` photos. Files are
//! written through `dalica-fs`, like everything else.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_core::{PhotoId, SystemClock};
use dalica_fs::Expect;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::session::Session;

/// The temporary directories of a test library: keep them alive while the session is used.
pub(crate) struct Dirs {
    pub library: tempfile::TempDir,
    pub photos: tempfile::TempDir,
}

pub(crate) fn open(dir: &Path) -> Session {
    let mut s = Session::open(dir, Arc::new(SystemClock)).unwrap();
    s.wait_idle();
    s
}

/// A session whose one watched folder holds `n` photos `IMG_0001.jpg`…, ids in name order.
pub(crate) fn library(n: usize) -> (Dirs, Session, Vec<PhotoId>) {
    let dirs = Dirs { library: tempfile::tempdir().unwrap(), photos: tempfile::tempdir().unwrap() };
    let folder = dirs.photos.path().join("Lisbon");
    dalica_fs::create_dir_all(&folder).unwrap();
    for i in 0..n {
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i as u64, 120, 80), 80, None).unwrap();
        dalica_fs::replace(&folder.join(format!("IMG_{:04}.jpg", i + 1)), &bytes, Expect::Absent).unwrap();
    }
    let mut s = open(dirs.library.path());
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let mut photos: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
    photos.sort();
    (dirs, s, photos.into_iter().map(|(_, id)| id).collect())
}

/// Parameters no command accepts: each must give an error (never a panic) and change nothing.
pub(crate) fn hostile_params() -> Vec<Value> {
    vec![
        json!([]),
        json!("text"),
        json!(42),
        json!(true),
        json!({ "zzz": 1 }),
        json!({ "ids": "x" }),
        json!({ "ids": [-1] }),
        json!({ "ids": [1.5] }),
        json!({ "ids": [18_446_744_073_709_551_615u64, "x"] }),
        json!({ "path": 7 }),
        json!({ "limit": -1 }),
        json!({ "on": "yes", "ids": [] }),
        serde_json::from_str::<Value>(&format!("{}{}", "[".repeat(100), "]".repeat(100))).unwrap_or(Value::Null),
    ]
}

/// Runs `id` with every hostile parameter set: an error each time, never `Internal`, and no
/// change to the model or the undo stack.
pub(crate) fn hostile(s: &mut Session, id: &str) {
    let (model, undo) = (s.library().model().clone(), s.undo_stack().len());
    for p in hostile_params() {
        let r = s.execute(id, &p);
        assert!(r.is_err(), "{id} accepted {p}");
        assert!(!matches!(r, Err(CmdError::Internal(_))), "{id} panicked on {p}");
        assert_eq!(s.library().model(), &model, "{id} changed the model on {p}");
        assert_eq!(s.undo_stack().len(), undo, "{id} recorded a step on {p}");
    }
}

/// Test-only commands: `test.panic` panics, to show the dispatch guard.
pub(crate) fn specs() -> Vec<crate::registry::CommandSpec> {
    vec![crate::registry::CommandSpec {
        id: "test.panic",
        label: "Panic",
        menu: "",
        keys: &[],
        params: "{}",
        effect: crate::registry::Effect::Query,
        history: crate::registry::History::None,
        enabled: crate::registry::always,
        run: |_, _| panic!("a deliberate panic"),
    }]
}

#[allow(dead_code)]
pub(crate) fn path_of(s: &Session, id: PhotoId) -> PathBuf {
    let p = s.library().photo(id).unwrap();
    s.library().folder(p.folder).unwrap().path.join(&p.name)
}
