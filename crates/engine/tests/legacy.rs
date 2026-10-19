//! The previous organizer's data in the session (`roadmap.md` M14.13, `data-model.md` §9): two
//! legacy-only folders sharing an album and a person give one album and one person; the scan
//! writes nothing; the first change writes `.dalica.json` and the folder's source turns `both`;
//! "Read photo data left by the previous organizer" off takes the legacy-only data out and on
//! brings it back, writing nothing; the notice's count comes once; `legacy.inspect` reports each
//! source, and `photo.inspect` each photo's `legacyEdits`; the commands' graceful failures; the
//! owner thread makes no `fs` call. The fixtures are written here from the documented format,
//! their names from `dalica-sidecar`'s constant.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dalica_core::PhotoId;
use dalica_engine::{CmdError, Session};
use dalica_fs::Expect;
use dalica_sidecar::legacy::{folder_section, sidecar_names};
use serde_json::{Value, json};

use crate::common::{jpeg, off_owner, open, read_json};

/// Files with their bytes; a mismatch names the paths that differ, not the bytes.
#[derive(PartialEq, Eq)]
struct Files(BTreeMap<PathBuf, Vec<u8>>);

impl std::fmt::Debug for Files {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.0.iter().map(|(p, b)| (p.file_name().map(|n| n.len()), p.parent().and_then(|d| d.file_name()), b.len()))).finish()
    }
}

const ALBUM: &str = "f07f69266e7760526d5b1553c7f451fa";
const ZOE: &str = "0632e71e2ffd6c6d";

fn put(path: &Path, bytes: &[u8]) {
    if let Some(dir) = path.parent() {
        dalica_fs::create_dir_all(dir).unwrap();
    }
    dalica_fs::replace(path, bytes, Expect::Absent).unwrap();
}

/// `lines` as a legacy sidecar of `dir` (CRLF, as the legacy organizer writes it).
fn legacy(dir: &Path, lines: &[String]) {
    put(&dir.join(&sidecar_names()[0]), (lines.join("\r\n") + "\r\n").as_bytes());
}

/// The album's definition, as each folder that uses it holds it.
fn album() -> Vec<String> {
    vec![format!("[.album:{ALBUM}]"), "name=Garden 2015".into(), format!("token={ALBUM}")]
}

/// `Pictures/`: its own legacy sidecar names Zoë (no photos); `Garden/` and `Lake/` are
/// legacy-only, each with a photo showing Zoë in the album; `Home/` has only Dalica's file;
/// `Attic/` has both; `Plain/` has none.
struct Lib {
    _dir: tempfile::TempDir,
    root: PathBuf,
    s: Session,
}

impl Lib {
    fn new() -> Lib {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Pictures");
        legacy(&root, &["[encoding]".into(), "utf8=1".into(), "[Contacts2]".into(), "632e71e2ffd6c6d=Zoë Ann;;".into()]);
        let face = "faces=rect64(4000400080008000),632e71e2ffd6c6d;".to_string();
        for (i, name) in ["Garden", "Lake", "Home", "Attic", "Plain"].iter().enumerate() {
            let folder = root.join(name);
            put(&folder.join("a.jpg"), &jpeg(i as u64 * 2));
            put(&folder.join("b.jpg"), &jpeg(i as u64 * 2 + 1));
        }
        let mut garden = vec!["[a.jpg]".into(), "star=yes".into(), "caption=In the beds".into(), face.clone(), format!("albums={ALBUM}")];
        garden.extend([format!("[{}]", folder_section()), "description=Planting".into()]);
        garden.extend(album());
        legacy(&root.join("Garden"), &garden);
        let mut lake = vec![
            "[a.jpg]".into(),
            "keywords=water".into(),
            face,
            format!("albums={ALBUM}"),
            "geotag=33.770556,-84.293055".into(),
            "filters=sepia=1;redeye=1;".into(),
        ];
        lake.extend(album());
        legacy(&root.join("Lake"), &lake);
        let home = json!({ "dalica": { "version": 1 }, "photos": { "a.jpg": { "star": true } } });
        put(&root.join("Home").join(".dalica.json"), &serde_json::to_vec(&home).unwrap());
        put(&root.join("Attic").join(".dalica.json"), &serde_json::to_vec(&json!({ "dalica": { "version": 1 } })).unwrap());
        legacy(&root.join("Attic"), &["[a.jpg]".into(), "star=yes".into()]);
        let s = open(&dir.path().join("library"));
        Lib { _dir: dir, root, s }
    }

    fn photo(&self, folder: &str, name: &str) -> PhotoId {
        let lib = self.s.library();
        let f = lib.folder_by_path(&self.root.join(folder)).unwrap();
        lib.photo_by_name(f, name).unwrap().id
    }

    fn inspect(&mut self, params: Value) -> BTreeMap<String, Value> {
        let v = self.s.execute("legacy.inspect", &params).unwrap();
        v["folders"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| (f["path"].as_str().unwrap().rsplit(['/', '\\']).next().unwrap().to_string(), f.clone()))
            .collect()
    }

    /// Every file under the root but in the folders Dalica's own file describes (`Home`, `Attic`)
    /// with its bytes (read off the owner thread).
    fn files(&self) -> Files {
        let root = self.root.clone();
        off_owner(move || {
            let mut out = BTreeMap::new();
            let mut stack = vec![root];
            while let Some(dir) = stack.pop() {
                for e in std::fs::read_dir(&dir).unwrap() {
                    let p = e.unwrap().path();
                    let own = p.parent().and_then(|d| d.file_name()).is_some_and(|n| n == "Home" || n == "Attic");
                    if p.is_dir() {
                        stack.push(p);
                    } else if !own {
                        out.insert(p.clone(), std::fs::read(&p).unwrap());
                    }
                }
            }
            Files(out)
        })
    }

    fn set_option(&mut self, on: bool) {
        self.s.execute("options.set", &json!({ "values": { "files.readLegacyData": on } })).unwrap();
        self.s.wait_idle();
        self.s.flush().unwrap();
    }

    /// The album and person the legacy data names, by their legacy ids.
    fn album_and_person(&self) -> (Option<Vec<PhotoId>>, Option<Vec<PhotoId>>) {
        let lib = self.s.library();
        let album = lib.album_by_legacy(ALBUM).map(|a| {
            let mut m = lib.album_members(a).to_vec();
            m.sort();
            m
        });
        let person = lib.person_by_legacy(ZOE).map(|p| {
            let mut m = lib.person_photos(p).to_vec();
            m.sort();
            m
        });
        (album, person)
    }
}

#[test]
fn two_legacy_folders_give_one_album_and_one_person_and_the_option_takes_them_out_and_back() {
    let mut lib = Lib::new();
    let before = lib.files();
    let ui = dalica_fs::mark_ui_thread();
    lib.s.execute("folders.add", &json!({ "path": lib.root })).unwrap();
    lib.s.wait_idle();
    lib.s.flush().unwrap();
    let (ga, la) = (lib.photo("Garden", "a.jpg"), lib.photo("Lake", "a.jpg"));
    let mut both = vec![ga, la];
    both.sort();
    assert_eq!(lib.album_and_person(), (Some(both.clone()), Some(both.clone())), "one album and one person for both folders");
    let model = lib.s.library();
    assert_eq!(model.albums().count(), 1);
    assert_eq!(model.people().count(), 1);
    let zoe = model.person_by_legacy(ZOE).unwrap();
    assert_eq!(model.person(zoe).unwrap().name, "Zoë Ann", "named by the parent folder's sidecar");
    assert!(model.photo(ga).unwrap().star && model.info(ga).caption == "In the beds");
    assert_eq!(model.info(la).tags, ["water"]);
    assert!(model.geotag(la).is_some());
    assert_eq!(lib.s.undo_stack().len(), 0, "no undo step");
    assert_eq!(lib.files(), before, "the scan wrote nothing");
    // `photo.inspect` counts the recipe's entries kept as `legacy:` (M14.28).
    let shown = lib.s.execute("photo.inspect", &json!({ "ids": [la, ga] })).unwrap();
    assert_eq!((shown[0]["recipe"]["edits"].clone(), shown[0]["legacyEdits"].clone()), (json!(["sepia()", "legacy:redeye(1)"]), json!(1)));
    assert_eq!(shown[1]["legacyEdits"], json!(0));

    // The sources, and the notice once.
    let sources: BTreeMap<String, String> = lib.inspect(json!({})).into_iter().map(|(k, v)| (k, v["source"].as_str().unwrap().to_string())).collect();
    let expect = [("Attic", "both"), ("Garden", "legacy"), ("Lake", "legacy")].map(|(a, b)| (a.to_string(), b.to_string()));
    assert_eq!(sources, BTreeMap::from(expect));
    let (home, plain) =
        (lib.s.library().folder_by_path(&lib.root.join("Home")).unwrap(), lib.s.library().folder_by_path(&lib.root.join("Plain")).unwrap());
    let named = lib.inspect(json!({ "folders": [home, plain, home] }));
    assert_eq!((named.len(), named["Home"]["source"].clone(), named["Plain"]["source"].clone()), (2, json!("dalica"), json!("none")));
    let garden = &lib.inspect(json!({}))["Garden"];
    assert_eq!(
        (garden["starred"].clone(), garden["faces"].clone(), garden["albums"].clone(), garden["people"].clone()),
        (json!(1), json!(1), json!(1), json!(1))
    );
    assert_eq!(lib.s.legacy_notice(), Some(3), "the notice counts the folders with legacy data");
    assert_eq!(lib.s.legacy_notice(), None, "once");

    // Off: the legacy-only data leaves, with the album and the person; nothing written.
    lib.set_option(false);
    assert_eq!(lib.album_and_person(), (None, None));
    let model = lib.s.library();
    assert!(!model.photo(ga).unwrap().star && model.info(ga).caption.is_empty() && model.faces(ga).is_empty());
    assert!(model.info(la).tags.is_empty() && model.geotag(la).is_none());
    assert_eq!(lib.inspect(json!({})).keys().collect::<Vec<_>>(), Vec::<&String>::new(), "no folder holds legacy data");
    assert_eq!(lib.files(), before, "nothing written");

    // On: everything back; nothing written; the notice does not come again.
    lib.set_option(true);
    let (album, person) = lib.album_and_person();
    assert_eq!((album.map(|m| m.len()), person.map(|m| m.len())), (Some(2), Some(2)));
    let model = lib.s.library();
    assert!(model.photo(ga).unwrap().star && model.info(ga).caption == "In the beds" && model.faces(ga).len() == 1);
    assert_eq!(model.info(la).tags, ["water"]);
    assert_eq!(lib.files(), before, "nothing written");
    assert_eq!(lib.s.legacy_notice(), None);
    assert_eq!(lib.s.undo_stack().len(), 0, "no undo step");

    // The first change writes `.dalica.json` with the folder's data; its source turns `both`.
    let gb = lib.photo("Garden", "b.jpg");
    lib.s.execute("photo.star.set", &json!({ "ids": [gb], "on": true })).unwrap();
    lib.s.flush().unwrap();
    drop(ui);
    let written = read_json(&lib.root.join("Garden").join(".dalica.json")).expect("the first change wrote .dalica.json");
    assert_eq!(written["photos"]["b.jpg"]["star"], json!(true));
    assert_eq!(written["photos"]["a.jpg"]["caption"], json!("In the beds"), "with the folder's legacy data");
    let after = lib.files();
    let legacy_file = lib.root.join("Garden").join(&sidecar_names()[0]);
    assert_eq!(after.0.get(&legacy_file), before.0.get(&legacy_file), "the legacy file is untouched");
    assert_eq!(lib.inspect(json!({}))["Garden"]["source"], json!("both"));
    assert!(lib.s.library().legacy_folders().len() == 3);
}

#[test]
fn the_commands_fail_gracefully_and_the_notice_answered_stays_answered() {
    let mut lib = Lib::new();
    let root = lib.root.clone();
    let s = &mut lib.s;
    for bad in [json!({ "folders": "x" }), json!({ "folders": [-1] }), json!({ "other": 1 }), json!(7)] {
        assert!(matches!(s.execute("legacy.inspect", &bad), Err(CmdError::BadParams(_))), "{bad}");
    }
    assert!(matches!(s.execute("legacy.inspect", &json!({ "folders": [999] })), Err(CmdError::NotFound(_))));
    let empty = s.execute("legacy.inspect", &Value::Null).unwrap();
    assert_eq!((empty["folders"].clone(), empty["notice"].clone(), empty["readLegacyData"].clone()), (json!([]), json!(null), json!(true)));
    assert!(matches!(s.execute("legacy.noticeShown", &json!({ "x": 1 })), Err(CmdError::BadParams(_))));
    assert!(!s.settings().legacy().legacy_notice_shown, "a refused call changes nothing");
    assert_eq!(s.execute("legacy.noticeShown", &json!({})).unwrap(), json!({ "changed": true }));
    assert_eq!(s.execute("legacy.noticeShown", &json!({})).unwrap(), json!({ "changed": false }));
    assert!(s.settings().legacy().legacy_notice_shown);
    // Answered, the scan that finds legacy data gives no notice.
    s.execute("folders.add", &json!({ "path": root })).unwrap();
    s.wait_idle();
    assert_eq!(s.library().legacy_folders().len(), 3);
    assert_eq!(s.legacy_notice(), None);
    assert_eq!(s.execute("legacy.inspect", &json!({})).unwrap()["noticeShown"], json!(true));
}

/// A `.dalica.json` another program made in a legacy-only folder after the scan read it wins: the
/// first write writes nothing (reported unsaved), and the folder is read again from that file.
#[test]
fn a_dalica_file_that_appeared_before_the_first_write_wins_and_is_read() {
    let mut lib = Lib::new();
    lib.s.set_watching(dalica_engine::watching::WatchMode::Off);
    lib.s.execute("folders.add", &json!({ "path": lib.root })).unwrap();
    lib.s.wait_idle();
    let (la, lb) = (lib.photo("Lake", "a.jpg"), lib.photo("Lake", "b.jpg"));
    assert_eq!(lib.s.library().info(la).tags, ["water"]);
    let theirs = serde_json::to_vec(&json!({ "dalica": { "version": 1 }, "photos": { "a.jpg": { "caption": "From elsewhere" } } })).unwrap();
    let file = lib.root.join("Lake").join(".dalica.json");
    off_owner(|| put(&file, &theirs));
    let ui = dalica_fs::mark_ui_thread();
    lib.s.execute("photo.star.set", &json!({ "ids": [lb], "on": true })).unwrap();
    assert!(matches!(lib.s.flush(), Err(CmdError::Unsaved(_))), "the first write wrote nothing");
    lib.s.wait_idle();
    lib.s.flush().unwrap();
    drop(ui);
    let model = lib.s.library();
    assert_eq!((model.info(la).caption.as_str(), model.info(la).tags.is_empty()), ("From elsewhere", true), "read from their file");
    assert!(model.faces(la).is_empty() && model.geotag(la).is_none());
    assert_eq!(crate::common::read(&file), theirs, "their file is kept byte for byte");
    assert_eq!(lib.inspect(json!({}))["Lake"]["source"], json!("both"));
    let (album, person) = lib.album_and_person();
    assert_eq!((album.map(|m| m.len()), person.map(|m| m.len())), (Some(1), Some(1)), "Garden still holds them");
}
