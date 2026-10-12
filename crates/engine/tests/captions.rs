//! `photo.caption.set` on a temporary library (`roadmap.md` M6.14; `organizing.md` §5.3,
//! `data-model.md` §3): the caption trimmed, capped and cleared by "", one undo step for any
//! number of photos and none when nothing changes, undo and redo exact, the sidecar after the
//! command and after undo, and captions surviving a restart and a rebuild of the index.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_core::{PhotoId, SystemClock};
use dalica_engine::{CacheDir, CmdError, Options, Session, ThumbOptions};
use dalica_fs::Expect;
use dalica_library::{MAX_CAPTION, PhotoInfo};
use serde_json::{Value, json};

fn open(library: &Path) -> Session {
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, workers: 1, ..ThumbOptions::default() } };
    let mut s = Session::open_with(library, Arc::new(SystemClock), &options).unwrap();
    s.wait_idle();
    s
}

/// A library watching one folder `Lisbon` of `n` photos `IMG_0001.jpg`…; ids in name order.
fn setup(n: usize) -> (tempfile::TempDir, tempfile::TempDir, PathBuf, Session, Vec<PhotoId>) {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let folder = disk.path().join("Lisbon");
    dalica_fs::create_dir_all(&folder).unwrap();
    for i in 0..n {
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i as u64, 96, 64), 80, None).unwrap();
        dalica_fs::replace(&folder.join(format!("IMG_{:04}.jpg", i + 1)), &bytes, Expect::Absent).unwrap();
    }
    let mut s = open(lib.path());
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let ids = by_name(&s).into_values().collect();
    (lib, disk, folder, s, ids)
}

fn by_name(s: &Session) -> BTreeMap<String, PhotoId> {
    s.library().photos().map(|p| (p.name.clone(), p.id)).collect()
}

/// What the user said about each photo, by name (ids change in a rebuild).
fn infos(s: &Session) -> BTreeMap<String, PhotoInfo> {
    s.library().photos().map(|p| (p.name.clone(), s.library().info(p.id).clone())).filter(|(_, i)| !i.is_empty()).collect()
}

/// The folder's sidecar after every pending write landed (`Null` when there is none).
fn sidecar(s: &mut Session, folder: &Path) -> Value {
    s.flush().unwrap();
    std::fs::read(folder.join(".dalica.json")).map_or(Value::Null, |b| serde_json::from_slice(&b).unwrap())
}

fn captions(v: &Value) -> BTreeMap<String, String> {
    v["photos"]
        .as_object()
        .map(|o| o.iter().filter_map(|(k, e)| Some((k.clone(), e.get("caption")?.as_str()?.to_string()))).collect())
        .unwrap_or_default()
}

#[test]
fn a_caption_is_trimmed_and_set_on_every_photo_in_one_step_and_undo_redo_are_exact() {
    let (_l, _d, folder, mut s, ids) = setup(3);
    let m0 = s.library().model().clone();
    let v = s.execute("photo.caption.set", &json!({ "ids": [ids[0], ids[2], ids[0]], "caption": "  Tram 28 at sunset \n" })).unwrap();
    assert_eq!(v, json!({ "ids": [ids[0], ids[2]], "caption": "Tram 28 at sunset", "changed": 2 }));
    assert_eq!(s.library().caption(ids[0]), "Tram 28 at sunset");
    assert_eq!(s.library().caption(ids[1]), "");
    assert_eq!(s.undo_stack().len(), 1);
    assert_eq!(s.undo_stack().undo_label(), Some("Set Caption"));
    let after = sidecar(&mut s, &folder);
    assert_eq!(
        captions(&after),
        BTreeMap::from([("IMG_0001.jpg".into(), "Tram 28 at sunset".into()), ("IMG_0003.jpg".into(), "Tram 28 at sunset".into())])
    );
    let m1 = s.library().model().clone();

    // The same caption again changes nothing and records no step.
    let v = s.execute("photo.caption.set", &json!({ "ids": [ids[0]], "caption": "Tram 28 at sunset" })).unwrap();
    assert_eq!(v["changed"], json!(0));
    assert_eq!(s.undo_stack().len(), 1);

    // A different caption on a mixed selection, then "" clears.
    s.execute("photo.caption.set", &json!({ "ids": [ids[0], ids[1]], "caption": "Alfama" })).unwrap();
    let m2 = s.library().model().clone();
    s.execute("photo.caption.set", &json!({ "ids": [ids[0], ids[1], ids[2]], "caption": "   " })).unwrap();
    assert!(infos(&s).is_empty(), "an empty caption clears it: {:?}", infos(&s));
    assert!(captions(&sidecar(&mut s, &folder)).is_empty());

    for want in [&m2, &m1, &m0] {
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.library().model(), want);
    }
    let undone = sidecar(&mut s, &folder);
    assert!(undone["photos"].as_object().is_none_or(|o| o.is_empty()), "after undo no caption and no empty entry: {undone}");
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.library().model(), &m1);
    assert_eq!(sidecar(&mut s, &folder), after, "redo writes the same sidecar again");
}

#[test]
fn a_caption_past_the_cap_is_cut_at_a_character_boundary() {
    let (_l, _d, _f, mut s, ids) = setup(1);
    // "é" is two bytes: the cap falls in the middle of one.
    let long = format!("a{}", "é".repeat(MAX_CAPTION));
    let v = s.execute("photo.caption.set", &json!({ "ids": [ids[0]], "caption": long })).unwrap();
    let got = s.library().caption(ids[0]);
    assert!(got.len() <= MAX_CAPTION && got.len() >= MAX_CAPTION - 1, "{}", got.len());
    assert!(long.starts_with(got));
    assert_eq!(v["caption"], json!(got));
    // Newlines inside a caption are kept (a caption may run over several lines).
    s.execute("photo.caption.set", &json!({ "ids": [ids[0]], "caption": "line one\nline two" })).unwrap();
    assert_eq!(s.library().caption(ids[0]), "line one\nline two");
}

#[test]
fn disabled_and_hostile_params_change_nothing() {
    let (_l, _d, _f, mut s, ids) = setup(1);
    let m = s.library().model().clone();
    assert_eq!(s.execute("photo.caption.set", &json!({ "ids": [], "caption": "x" })), Err(CmdError::Disabled("Select some photos first.".into())));
    let spec = *s.spec("photo.caption.set").unwrap();
    assert!((spec.enabled)(&s, &json!({ "ids": [], "caption": "x" })).is_err());
    assert!((spec.enabled)(&s, &json!({ "ids": [ids[0]], "caption": "x" })).is_ok());
    for bad in [
        json!({ "ids": [999], "caption": "x" }),
        json!({ "ids": [ids[0]] }),
        json!({ "caption": "x" }),
        json!({ "ids": [ids[0]], "caption": 7 }),
        json!({ "ids": [ids[0]], "caption": ["x"] }),
        json!({ "ids": [ids[0]], "caption": "x", "extra": true }),
        json!({ "ids": [-1], "caption": "x" }),
        json!({ "ids": "all", "caption": "x" }),
        json!("caption"),
    ] {
        assert!(s.execute("photo.caption.set", &bad).is_err(), "{bad}");
    }
    let too_many: Vec<u64> = vec![ids[0].get(); 100_001];
    assert!(matches!(s.execute("photo.caption.set", &json!({ "ids": too_many, "caption": "x" })), Err(CmdError::Limit(_))));
    assert_eq!(s.library().model(), &m);
    assert_eq!(s.undo_stack().len(), 0);
}

#[test]
fn captions_survive_a_restart_and_a_rebuild_of_the_index() {
    let (lib, _d, folder, mut s, ids) = setup(3);
    s.execute("photo.caption.set", &json!({ "ids": [ids[0]], "caption": "Tram 28" })).unwrap();
    s.execute("photo.caption.set", &json!({ "ids": [ids[2]], "caption": "Miradouro 🌅" })).unwrap();
    let want = infos(&s);
    assert_eq!(want.len(), 2);
    let written = captions(&sidecar(&mut s, &folder));
    drop(s);
    let mut s = open(lib.path());
    assert_eq!(infos(&s), want, "after a restart (journal and snapshot)");
    s.execute("library.rebuild", &json!({})).unwrap();
    s.wait_idle();
    assert_eq!(by_name(&s).len(), 3);
    // Another change in the folder after the rebuild rewrites its sidecar: the captions stay.
    let other = by_name(&s)["IMG_0002.jpg"];
    s.execute("photo.star.set", &json!({ "ids": [other], "on": true })).unwrap();
    assert_eq!(captions(&sidecar(&mut s, &folder)), written, "a write after a rebuild keeps the captions in the sidecar");
    assert_eq!(infos(&s).into_iter().map(|(n, i)| (n, i.caption)).collect::<BTreeMap<_, _>>(), written, "after a rebuild of the index");
}
