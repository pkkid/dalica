//! The options catalogue (`crates/engine/src/options.rs`, `roadmap.md` M10.1, M11.1) against
//! `menus-and-shortcuts.md` §5: the six tabs shown are the catalogue's five and the shortcut
//! editor; every option of the General, File Types, Slideshow, Photo Viewer and Export tables is in
//! the catalogue once, on its tab, and nothing else is; keys are unique; each default is what its
//! setting's accessor reads from an empty file (and a bool's is the table's on / off); and each
//! option that can be changed, stored under its key with each of its choices, is read back by that
//! accessor.

use std::collections::BTreeMap;
use std::path::Path;

use dalica_engine::options::{Initial, Kind, OptionSpec, Tab, catalogue};
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

/// The tabs §5 names, in order, with the milestones' tabs still to come (Network, People) left out.
#[test]
fn the_tabs_shown_are_the_catalogues_and_the_shortcut_editor() {
    let doc = doc();
    let options = &doc[doc.find("## 5. Options").unwrap()..];
    let start = options.find("Tabs: **").unwrap() + "Tabs: **".len();
    let list = options[start..].split("**").next().unwrap().replace('\n', " ");
    let named: Vec<&str> = list.split('·').map(str::trim).collect();
    let shown: Vec<&str> = named.into_iter().filter(|t| !["Network", "People"].contains(t)).collect();
    let mut want: Vec<&str> = Tab::ALL.iter().map(|t| t.label()).collect();
    want.push("Keyboard Shortcuts");
    assert_eq!(shown, want, "the catalogue's tabs in the dialog's order, then the shortcut editor");
    for tab in Tab::ALL {
        assert_eq!(Tab::from_id(tab.id()), Some(tab));
        assert!(catalogue().iter().any(|o| o.tab == tab), "{tab:?} has options");
    }
}

#[test]
fn every_option_of_the_five_tabs_is_in_the_catalogue_once() {
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
            if let Initial::Bool(on) = o.default {
                assert_eq!(default, if on { "on" } else { "off" }, "{key}");
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
            Kind::Choice(choices) => choices.iter().map(|c| json!(c.0)).filter(|c| *c != o.default_value()).collect(),
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
    for key in
        ["interface.language", "files.readLegacyData", "slideshow.music", "slideshow.musicFolder", "slideshow.videoSound", "viewer.defaultViewer"]
    {
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
