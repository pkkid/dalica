//! The options catalogue (`crates/engine/src/options.rs`, `roadmap.md` M10.1) against
//! `menus-and-shortcuts.md` §5: every option of the General, File Types and Export tables is in the
//! catalogue once, on its tab, and nothing else is; keys are unique; each default is what its
//! setting's accessor reads from an empty file (and a bool's is the table's on / off); and each
//! option that can be changed, stored under its key, is read back by that accessor.

use std::collections::BTreeMap;
use std::path::Path;

use dalica_engine::options::{Initial, Kind, Tab, catalogue};
use dalica_engine::settings::Settings;
use serde_json::json;

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

#[test]
fn every_option_of_the_three_tabs_is_in_the_catalogue_once() {
    let doc = doc();
    let options = &doc[doc.find("## 5. Options").unwrap()..];
    let mut keys: BTreeMap<&str, usize> = BTreeMap::new();
    for o in catalogue() {
        *keys.entry(o.key).or_default() += 1;
    }
    assert!(keys.values().all(|&n| n == 1), "keys are unique: {keys:?}");
    for (tab, title) in [(Tab::General, "General"), (Tab::FileTypes, "File Types"), (Tab::Export, "Export")] {
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
        let other = match o.kind {
            Kind::Bool => json!(o.default != Initial::Bool(true)),
            Kind::Int { max, .. } => json!(max),
            Kind::Scale { .. } => json!(150),
            Kind::Choice(choices) => json!(choices.last().unwrap().0),
            Kind::Text { .. } => json!("© Ana"),
            Kind::Folder => json!(folder),
        };
        assert_ne!(other, o.default_value(), "{}", o.key);
        let mut st = Settings::parse(br#"{"future": 1}"#).unwrap();
        o.store(&mut st, &other).unwrap();
        let back = Settings::parse(&st.print()).unwrap();
        assert_eq!((o.read)(&back), other, "{}: stored under its key, read by its accessor", o.key);
        assert!(back.problems().is_empty(), "{}: {:?}", o.key, back.problems());
        let (object, field) = o.place();
        assert_eq!(back.get(object).and_then(|v| v.get(field)), Some(&other), "{}", o.key);
        assert_eq!(back.get("future"), Some(&json!(1)));
        let row = o.to_json(&back);
        assert_eq!((row["key"].as_str(), &row["value"], &row["default"]), (Some(o.key), &other, &o.default_value()), "{}", o.key);
    }
    // The options whose feature is still to come say why, and the HEIC row follows the build.
    let reasons: Vec<(&str, &str)> = catalogue().iter().filter_map(|o| o.unavailable.map(|r| (o.key, r))).collect();
    for key in ["interface.language", "import.detectDuplicates", "files.readLegacyData", "import.folder"] {
        assert!(reasons.iter().any(|(k, r)| *k == key && r.starts_with("Arrives with")), "{key}: {reasons:?}");
    }
    let heif = catalogue().iter().find(|o| o.key == "fileTypes.heif").unwrap();
    assert_eq!(heif.available(), cfg!(feature = "heif"));
}
