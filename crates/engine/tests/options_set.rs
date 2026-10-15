//! `options.list` and `options.set` (`roadmap.md` M10.5; `menus-and-shortcuts.md` §5, §1.9;
//! `design-system.md` §11.3) on a temporary library: each option round-trips through the two
//! commands and a restart with the keys `settings.json` holds that Dalica does not know kept; a
//! batch with one bad value changes nothing; a read-only settings file fails with its reason and
//! the old values stay; the options that apply at the next launch are listed as such. File Types
//! and the threshold, which rescan the watched folders, are in `file_types.rs`; hostile parameters
//! are the every-command loop's and `panic_hunt`'s.

use std::collections::BTreeMap;
use std::path::Path;

use dalica_engine::CmdError;
use dalica_engine::Session;
use dalica_engine::options::{Applies, Initial, Kind, OptionSpec, catalogue};
use dalica_engine::settings::FILE;
use dalica_fs::Expect;
use serde_json::{Map, Value, json};

use crate::common::{off_owner, open, read, read_json};

/// A value of `o` other than its default.
fn other(o: &OptionSpec, folder: &Path) -> Value {
    match o.kind {
        Kind::Bool => json!(o.default != Initial::Bool(true)),
        Kind::Int { max, .. } => json!(max),
        Kind::Scale { .. } => json!(150),
        Kind::Choice(choices) => json!(choices.last().unwrap().0),
        Kind::Text { .. } => json!("© Ana"),
        Kind::Folder => json!(folder),
    }
}

/// `options.set` of `values` (`(key, value)` pairs).
fn set(s: &mut Session, values: &[(&str, Value)]) -> Result<Value, CmdError> {
    let values: Map<String, Value> = values.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
    s.execute("options.set", &json!({ "values": values }))
}

/// The rows of `options.list {tab?}`, by key.
fn rows(s: &mut Session, tab: Option<&str>) -> BTreeMap<String, Value> {
    let params = tab.map_or(Value::Null, |t| json!({ "tab": t }));
    let v = s.execute("options.list", &params).unwrap();
    v["options"].as_array().unwrap().iter().map(|r| (r["key"].as_str().unwrap().to_string(), r.clone())).collect()
}

fn set_read_only(path: &Path, read_only: bool) {
    let path = path.to_path_buf();
    off_owner(move || {
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            perms.set_mode(if read_only { 0o444 } else { 0o644 });
        }
        #[cfg(not(unix))]
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(read_only);
        std::fs::set_permissions(&path, perms).unwrap();
    });
}

#[test]
fn each_option_round_trips_through_set_and_list_with_unknown_keys_kept() {
    let (lib, out) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let file = lib.path().join(FILE);
    dalica_fs::replace(&file, br#"{"future": {"x": 1}, "interface": {"later": 2}}"#, Expect::Absent).unwrap();
    let mut s = open(lib.path());
    let mut set_to = BTreeMap::new();
    for o in catalogue().iter().filter(|o| o.available()) {
        let value = other(o, out.path());
        let v = set(&mut s, &[(o.key, value.clone())]).unwrap();
        let restart = if o.applies == Applies::Restart { json!([o.key]) } else { json!([]) };
        assert_eq!(v, json!({ "changed": [o.key], "restart": restart, "rescan": o.applies == Applies::Rescan, "job": null }), "{}", o.key);
        assert_eq!(set(&mut s, &[(o.key, value.clone())]).unwrap()["changed"], json!([]), "{}: the same value again changes nothing", o.key);
        let tab = rows(&mut s, Some(o.tab.id()));
        assert!(tab.values().all(|r| r["tab"] == o.tab.id()), "{}: only its tab", o.key);
        assert_eq!(tab[o.key]["value"], value, "{}", o.key);
        set_to.insert(o.key, value);
    }
    s.flush().unwrap();
    let doc = read_json(&file).unwrap();
    assert_eq!((&doc["future"], &doc["interface"]["later"]), (&json!({ "x": 1 }), &json!(2)), "unknown keys kept: {doc}");

    // A restart reads every value back; the options still to come keep their defaults.
    drop(s);
    let mut s = open(lib.path());
    let all = rows(&mut s, None);
    assert_eq!(all.len(), catalogue().len());
    for o in catalogue() {
        let want = set_to.get(o.key).cloned().unwrap_or_else(|| o.default_value());
        assert_eq!(all[o.key]["value"], want, "{}", o.key);
    }

    // Of several changes, those that apply at the next launch are listed apart.
    let v = set(&mut s, &[("interface.tooltips", json!(true)), ("interface.systemTitleBar", json!(false))]).unwrap();
    assert_eq!((&v["changed"], &v["restart"]), (&json!(["interface.tooltips", "interface.systemTitleBar"]), &json!(["interface.systemTitleBar"])));
    assert!(matches!(s.execute("options.list", &json!({ "tab": "slideshow" })), Err(CmdError::BadParams(m)) if m.contains("\"general\"")));
}

#[test]
fn a_batch_with_one_bad_value_changes_nothing() {
    let lib = tempfile::tempdir().unwrap();
    let file = lib.path().join(FILE);
    let mut s = open(lib.path());
    set(&mut s, &[("interface.tooltips", json!(false))]).unwrap();
    s.flush().unwrap();
    let (bytes, before) = (read(&file), s.settings().clone());
    let fine = ("interface.animations", json!(false));
    for (key, bad) in [
        ("export.size", json!(99_999)),
        ("interface.uiScale", json!(80)),
        ("interface.fontSize", json!(true)),
        ("export.location", json!("relative/folder")),
        ("no.suchOption", json!(1)),
        ("import.folder", Value::Null),
    ] {
        match set(&mut s, &[fine.clone(), (key, bad.clone())]) {
            Err(CmdError::BadParams(m)) => assert!(m.contains(key), "{key}: {m}"),
            other => panic!("{key} = {bad}: {other:?}"),
        }
        assert_eq!(s.settings(), &before, "{key}: nothing changed");
    }
    s.flush().unwrap();
    assert_eq!(read(&file), bytes, "nothing written");
}

#[test]
fn a_read_only_settings_file_fails_with_its_reason_and_the_old_values_stay() {
    let lib = tempfile::tempdir().unwrap();
    let file = lib.path().join(FILE);
    let mut s = open(lib.path());
    set(&mut s, &[("interface.tooltips", json!(false))]).unwrap();
    s.flush().unwrap();
    let bytes = read(&file);

    set_read_only(&file, true);
    match set(&mut s, &[("interface.tooltips", json!(true)), ("export.size", json!(2000))]) {
        Err(CmdError::Disabled(m)) => assert!(m.contains("settings.json is read-only") && m.contains("Nothing was changed"), "{m}"),
        other => panic!("{other:?}"),
    }
    let after = rows(&mut s, None);
    assert_eq!((&after["interface.tooltips"]["value"], &after["export.size"]["value"]), (&json!(false), &json!(1600)), "the old values stay");
    assert_eq!(read(&file), bytes);

    set_read_only(&file, false);
    set(&mut s, &[("interface.tooltips", json!(true))]).unwrap();
    s.flush().unwrap();
    assert_eq!(read_json(&file).unwrap()["interface"]["tooltips"], json!(true));
}
