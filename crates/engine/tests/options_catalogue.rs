//! The options catalogue (`crates/engine/src/options.rs`, `roadmap.md` M10.1, M11.1, M13.3, M14.5, M15.4) against
//! `menus-and-shortcuts.md` §5: the eight tabs shown are the catalogue's seven and the shortcut
//! editor; every option of the General, File Types, Slideshow, Network, People, Photo Viewer and
//! Export tables is in the catalogue once, on its tab, and nothing else is; keys are unique; each
//! default is what its setting's accessor reads from an empty file (and a bool's is the table's
//! on / off, a number's the table's number); each option that can be changed, stored under its key
//! with each of its choices, is read back by that accessor; and the People rows and the Network
//! rows of reserved decision 4 (M13.20) can be changed, the network log level still waiting; and
//! "Read photo data left by the previous organizer" can be changed (M14.5); Language lists the
//! languages the app gives and can be changed, the External editor is a text row (M15.22), and the
//! Photo Viewer's File Types is a list still waiting on the OS integration (M15.4).

use std::collections::BTreeMap;
use std::path::Path;

use dalica_engine::map_tiles::{PROXY_UNAVAILABLE, TILES_UNAVAILABLE};
use dalica_engine::options::{Applies, Initial, Kind, OptionSpec, Tab, catalogue, find, on_tab};
use dalica_engine::settings::Settings;
use serde_json::{Value, json};

/// The rows of §5's table under `### <title>`: `(key, default)`, the key from the third column.
fn table(doc: &str, title: &str) -> Vec<(String, String)> {
    let start = doc.find(&format!("\n### {title}\n")).unwrap_or_else(|| panic!("§5 has a {title} table"));
    let section = &doc[start + 1..];
    let end = section[4..].find("\n### ").map_or(section.len(), |e| e + 4);
    section[..end]
        .lines()
        .filter(|l| l.starts_with('|') && !l.starts_with("|---") && !l.starts_with("| Group"))
        .map(|l| {
            let cells: Vec<&str> = l.split('|').map(str::trim).collect();
            (cells[3].trim_matches('`').to_string(), cells[4].to_string())
        })
        .collect()
}

fn doc() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("docs").join("menus-and-shortcuts.md");
    std::fs::read_to_string(path).unwrap()
}

/// The tabs §5 names, in order.
#[test]
fn the_tabs_shown_are_the_catalogues_and_the_shortcut_editor() {
    let doc = doc();
    let options = &doc[doc.find("## 5. Options").unwrap()..];
    let start = options.find("Tabs: **").unwrap() + "Tabs: **".len();
    let list = options[start..].split("**").next().unwrap().replace('\n', " ");
    let shown: Vec<&str> = list.split('·').map(str::trim).collect();
    let mut want: Vec<&str> = Tab::ALL.iter().map(|t| t.label()).collect();
    want.push("Keyboard Shortcuts");
    assert_eq!(shown, want, "the catalogue's tabs in the dialog's order, then the shortcut editor");
    for tab in Tab::ALL {
        assert_eq!(Tab::from_id(tab.id()), Some(tab));
        assert!(catalogue().iter().any(|o| o.tab == tab), "{tab:?} has options");
    }
}

#[test]
fn every_option_of_the_seven_tabs_is_in_the_catalogue_once() {
    let doc = doc();
    let options = &doc[doc.find("## 5. Options").unwrap()..];
    let mut keys: BTreeMap<&str, usize> = BTreeMap::new();
    for o in catalogue() {
        *keys.entry(o.key).or_default() += 1;
    }
    assert!(keys.values().all(|&n| n == 1), "keys are unique: {keys:?}");
    for tab in Tab::ALL {
        let title = tab.label();
        let rows = table(options, title);
        let documented: Vec<&str> = rows.iter().map(|(k, _)| k.as_str()).collect();
        let listed: Vec<&str> = catalogue().iter().filter(|o| o.tab == tab).map(|o| o.key).collect();
        assert_eq!(listed, documented, "{title}: the catalogue lists §5's options, in its order");
        for (key, default) in &rows {
            let o = catalogue().iter().find(|o| o.key == key).unwrap();
            match o.default {
                Initial::Bool(on) => assert_eq!(default, if on { "on" } else { "off" }, "{key}"),
                Initial::Int(n) => assert!(default.starts_with(&n.to_string()), "{key}: {default} is not {n}"),
                Initial::Str(_) | Initial::List(_) | Initial::Null => {}
            }
        }
    }
}

#[test]
fn each_default_is_its_accessors_and_each_option_is_read_back_from_its_key() {
    let empty = Settings::default();
    let folder = std::env::temp_dir().join("Exports");
    for o in catalogue() {
        assert_eq!((o.read)(&empty), o.default_value(), "{}: the default is the accessor's", o.key);
        assert_eq!(o.check(&o.default_value()), Ok(o.default_value()), "{}: the default is a value it takes", o.key);
        if !o.available() {
            continue;
        }
        let others = match o.kind {
            Kind::Bool => vec![json!(o.default != Initial::Bool(true))],
            Kind::Int { max, .. } => vec![json!(max)],
            Kind::Scale { .. } => vec![json!(150)],
            Kind::Choice(_) => o.choices(&empty).into_iter().map(|c| json!(c.0)).filter(|c| *c != o.default_value()).collect(),
            Kind::Text { .. } => vec![json!("© Ana")],
            Kind::Folder => vec![json!(folder)],
        };
        assert!(!others.is_empty(), "{}", o.key);
        for other in others {
            assert_ne!(other, o.default_value(), "{}", o.key);
            stored_and_read_back(o, &other);
        }
    }
    // The options whose feature is still to come say why, and the HEIC row follows the build.
    let reasons: Vec<(&str, &str)> = catalogue().iter().filter_map(|o| o.unavailable.map(|r| (o.key, r))).collect();
    for key in ["slideshow.music", "slideshow.musicFolder", "slideshow.videoSound", "viewer.defaultViewer"] {
        assert!(reasons.iter().any(|(k, r)| *k == key && r.starts_with("Arrives with")), "{key}: {reasons:?}");
    }
    for key in ["import.detectDuplicates", "import.folder"] {
        assert!(catalogue().iter().any(|o| o.key == key && o.available()), "{key} can be changed now that Import exists");
    }
    let heif = catalogue().iter().find(|o| o.key == "fileTypes.heif").unwrap();
    assert_eq!(heif.available(), cfg!(feature = "heif"));
}

/// `other` stored under `o`'s key reads back through its accessor, every other key kept.
fn stored_and_read_back(o: &OptionSpec, other: &Value) {
    let mut st = Settings::parse(br#"{"future": 1}"#).unwrap();
    o.store(&mut st, other).unwrap();
    let back = Settings::parse(&st.print()).unwrap();
    assert_eq!(&(o.read)(&back), other, "{}: stored under its key, read by its accessor", o.key);
    assert!(back.problems().is_empty(), "{}: {:?}", o.key, back.problems());
    let (object, field) = o.place();
    assert_eq!(back.get(object).and_then(|v| v.get(field)), Some(other), "{}", o.key);
    assert_eq!(back.get("future"), Some(&json!(1)));
    let row = o.to_json(&back);
    assert_eq!((row["key"].as_str(), &row["value"], &row["default"]), (Some(o.key), other, &o.default_value()), "{}", o.key);
}

/// M13's tabs: the People rows can be changed and their sliders take 50–95 in steps of 5; the map
/// and proxy rows of the Network tab can be changed since reserved decisions 4 and 9 were taken
/// (`map_tiles` gives no reason), and the network log level waits on the network log.
#[test]
fn the_people_and_network_rows_can_be_changed_but_the_network_log() {
    let people: Vec<&str> = on_tab(Tab::People).map(|o| o.key).collect();
    assert_eq!(
        people,
        ["people.findFaces", "people.suggestNames", "people.suggestionConfidence", "people.groupingConfidence", "people.storeNamesInPhotos"]
    );
    assert!(on_tab(Tab::People).all(OptionSpec::available), "Options ▸ People can be changed");
    for key in ["people.suggestionConfidence", "people.groupingConfidence"] {
        let o = find(key).unwrap();
        for ok in [50, 55, 85, 95] {
            assert_eq!(o.check(&json!(ok)), Ok(json!(ok)), "{key} {ok}");
        }
        for bad in [json!(45), json!(87), json!(100), json!(85.5), json!("85")] {
            assert!(o.check(&bad).unwrap_err().ends_with("from 50 to 95 in steps of 5"), "{key} {bad}");
        }
    }
    let reasons: Vec<(&str, Option<&str>)> = on_tab(Tab::Network).map(|o| (o.key, o.unavailable)).collect();
    assert_eq!(
        reasons,
        [
            ("network.allowMapTiles", TILES_UNAVAILABLE),
            ("network.tileServer", TILES_UNAVAILABLE),
            ("network.tileAttribution", TILES_UNAVAILABLE),
            ("network.geocoder", TILES_UNAVAILABLE),
            ("network.proxy", PROXY_UNAVAILABLE),
            ("network.proxyHost", PROXY_UNAVAILABLE),
            ("network.proxyPort", PROXY_UNAVAILABLE),
            ("network.logLevel", Some("Arrives with the network log")),
        ]
    );
    assert_eq!((TILES_UNAVAILABLE, PROXY_UNAVAILABLE), (None, None));
    // Stored by hand, a row's value reads through its accessor.
    let mut st = Settings::default();
    st.merge("network", &[("allowMapTiles", Some(json!(true)))]);
    assert_eq!((find("network.allowMapTiles").unwrap().read)(&st), json!(true));
}

/// M14: the legacy row of General ▸ Files can be changed, its default is its accessor's, a change
/// rescans, and a stored value reads through `Settings::legacy`.
#[test]
fn the_legacy_row_can_be_changed() {
    let o = find("files.readLegacyData").unwrap();
    assert_eq!((o.tab, o.group, o.available(), o.applies), (Tab::General, "Files", true, Applies::Rescan));
    assert_eq!(o.default_value(), json!(Settings::default().legacy().read_legacy_data));
    let mut st = Settings::default();
    o.store(&mut st, &json!(false)).unwrap();
    assert!(!st.legacy().read_legacy_data);
    assert_eq!((o.read)(&st), json!(false));
    assert!(o.store(&mut st, &json!("off")).is_err());
}

/// M15: Language lists the languages the session was given (System first) and takes only those;
/// the External editor is a text row whose default is its accessor's (M15.22); the Photo Viewer's
/// File Types is a list of types, still waiting on the OS integration.
#[test]
fn the_m15_rows_language_external_editor_and_viewer_file_types() {
    let empty = Settings::default();
    let language = find("interface.language").unwrap();
    assert_eq!((language.available(), language.applies), (true, Applies::Now));
    let dir = tempfile::tempdir().unwrap();
    let mut s = crate::common::open(dir.path());
    let listed = |s: &mut dalica_engine::Session| -> Value {
        let rows = s.execute("options.list", &json!({ "tab": "general" })).unwrap();
        rows["options"].as_array().unwrap().iter().find(|r| r["key"] == "interface.language").unwrap()["choices"].clone()
    };
    assert_eq!(listed(&mut s), json!([{ "id": "system", "label": "System" }, { "id": "en", "label": "English" }]), "none given");
    s.set_languages(&[("en", "English"), ("th", "ไทย")]);
    assert_eq!(listed(&mut s), json!([{ "id": "system", "label": "System" }, { "id": "en", "label": "English" }, { "id": "th", "label": "ไทย" }]));
    s.execute("options.set", &json!({ "values": { "interface.language": "th" } })).unwrap();
    assert_eq!(s.settings().interface().language, "th");
    let refused = s.execute("options.set", &json!({ "values": { "interface.language": "fr" } })).unwrap_err().to_string();
    assert!(refused.contains("interface.language must be one of \"system\", \"en\", \"th\""), "{refused}");
    assert_eq!(s.settings().interface().language, "th");

    // The External editor: a text row of General ▸ Files, empty (the system's editor) by default,
    // as its accessor reads it, taking a program's path and nothing past MAX_PATH_BYTES.
    let editor = find("files.externalEditor").unwrap();
    assert_eq!((editor.tab, editor.group, editor.available(), editor.applies), (Tab::General, "Files", true, Applies::Now));
    assert_eq!(editor.kind, Kind::Text { max_bytes: dalica_engine::settings::MAX_PATH_BYTES });
    assert_eq!(editor.to_json(&empty)["type"], json!("text"));
    assert_eq!(editor.default_value(), json!(empty.external_editor()));
    assert_eq!(editor.default_value(), json!(""));
    let mut st = Settings::default();
    editor.store(&mut st, &json!("/usr/bin/gimp")).unwrap();
    assert_eq!(st.external_editor(), "/usr/bin/gimp", "read back through Settings::external_editor");
    let long = "x".repeat(dalica_engine::settings::MAX_PATH_BYTES + 1);
    for bad in [json!(long), json!(1), json!(null)] {
        assert!(editor.store(&mut st, &bad).is_err(), "{bad}");
    }
    assert_eq!(st.external_editor(), "/usr/bin/gimp", "nothing changed");

    let types = find("viewer.defaultViewer").unwrap();
    assert_eq!((types.many, types.available(), types.unavailable), (true, false, Some("Arrives with OS integration")));
    assert_eq!(types.to_json(&empty)["type"], json!("list"));
    assert_eq!(types.default_value(), json!([]));
    let ids: Vec<String> = types.choices(&empty).into_iter().map(|c| c.0).collect();
    let viewer: Vec<&str> = dalica_engine::settings::VIEWER_TYPES.iter().map(|t| t.id()).collect();
    assert_eq!(ids, viewer, "a choice per viewer type");
    assert_eq!(types.check(&json!(["raw", "jpeg", "raw"])), Ok(json!(["jpeg", "raw"])), "in the choices' order, each once");
    for bad in [json!("jpeg"), json!(["video"]), json!([1]), json!(true)] {
        assert!(types.check(&bad).unwrap_err().starts_with("viewer.defaultViewer must be a list of"), "{bad}");
    }
    let mut st = Settings::default();
    types.store(&mut st, &json!(["png"])).unwrap();
    assert_eq!((types.read)(&st), json!(["png"]), "read back through Settings::viewer");
}
