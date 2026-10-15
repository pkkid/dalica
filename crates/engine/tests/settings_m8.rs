//! M8's settings (`roadmap.md` M8.3; `data-model.md` §6, `creating.md` §2): Save's first-time
//! explanation and the last Save As folder and format, and the export's location, size, quality,
//! numbers, watermark and location removal round-trip through `settings.json` with unknown keys
//! kept; a value of the wrong type is its default and is reported by key (never by value) when the
//! library opens; the export location defaults to `<Pictures>/Dalica/Exports` and the projects
//! directory is its parent.

use std::path::{Path, PathBuf};

use crate::common::open;
use dalica_engine::settings::{
    CUSTOM_QUALITY_DEFAULT, EXPORT_SIZE_DEFAULT, EXPORT_SIZE_RANGE, ExportQuality, ExportSettings, FILE, MAX_PATH_BYTES, MAX_WATERMARK_BYTES,
    SaveFormat, SaveSettings, Settings, default_export_location,
};
use dalica_fs::Expect;
use serde_json::{Value, json};

fn write_settings(library: &Path, v: &Value) {
    dalica_fs::create_dir_all(library).unwrap();
    dalica_fs::replace(&library.join(FILE), serde_json::to_vec_pretty(v).unwrap().as_slice(), Expect::Absent).unwrap();
}

fn stored(library: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(library.join(FILE)).unwrap()).unwrap()
}

#[test]
fn nothing_stored_means_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    let save = s.settings().save();
    assert_eq!(save, SaveSettings::default());
    assert!(save.explain, "the first-time explanation is on");
    assert_eq!((save.last_folder, save.last_format), (None, SaveFormat::Jpeg));
    let e = s.settings().export();
    assert_eq!(e, ExportSettings::default());
    assert_eq!(e.location, None);
    assert_eq!((e.resize, e.size, e.long_side()), (true, 1600, Some(1600)), "resize to 1600");
    assert_eq!((e.quality, e.custom_quality, e.jpeg_quality()), (ExportQuality::Automatic, 75, None));
    assert!(!e.numbers && !e.watermark && e.watermark_text.is_empty() && !e.remove_location);
    assert!(s.notices().is_empty(), "{:?}", s.notices());
}

#[test]
fn the_export_location_defaults_under_pictures_and_the_projects_directory_stays_there() {
    let dir = tempfile::tempdir().unwrap();
    let pictures = dir.path().join("Pictures");
    let mut s = open(&dir.path().join("library"));
    s.set_pictures_dir(Some(pictures.clone()));
    let exports = pictures.join("Dalica").join("Exports");
    assert_eq!(default_export_location(&pictures), exports);
    assert_eq!(s.export_location(), Some(exports));
    assert_eq!(s.projects_dir(), Some(pictures.join("Dalica")));
    // A chosen location wins for exports; the projects directory stays `<Pictures>/Dalica`
    // (M9.28: `Desktop/Out` must not make the Desktop a projects folder).
    let chosen = dir.path().join("Desktop").join("Out");
    s.change_settings(|st| st.set_export(&ExportSettings { location: Some(chosen.clone()), ..ExportSettings::default() }));
    assert_eq!(s.export_location(), Some(chosen.clone()));
    assert_eq!(s.projects_dir(), Some(pictures.join("Dalica")));
    // No Pictures folder known: the chosen location still holds exports, and there is no projects
    // directory.
    s.set_pictures_dir(None);
    assert_eq!((s.export_location(), s.projects_dir()), (Some(chosen), None));
    // Nor any location when none is chosen.
    s.change_settings(|st| st.set_export(&ExportSettings::default()));
    assert_eq!((s.export_location(), s.projects_dir()), (None, None));
}

#[test]
fn every_new_setting_round_trips_with_unknown_keys_kept() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("library");
    write_settings(
        &lib,
        &json!({
            "future": {"c": 1},
            "save": {"futureFlag": true},
            "export": {"futurePreset": "web", "quality": "normal"},
            "edit": {"singleClickLeaves": true},
        }),
    );
    let mut s = open(&lib);
    assert!(s.notices().is_empty(), "{:?}", s.notices());
    assert_eq!(s.settings().export().quality, ExportQuality::Normal);
    let folder = dir.path().join("Saved As");
    let save = SaveSettings { explain: false, last_folder: Some(folder.clone()), last_format: SaveFormat::Png };
    let location = dir.path().join("Exports");
    let export = ExportSettings {
        location: Some(location.clone()),
        resize: false,
        size: 2048,
        quality: ExportQuality::Custom,
        custom_quality: 42,
        numbers: true,
        watermark: true,
        watermark_text: "© Ana Costa 2026".into(),
        remove_location: true,
    };
    let ui = dalica_fs::mark_ui_thread();
    s.change_settings(|st| {
        st.set_save(&save);
        st.set_export(&export);
    });
    s.flush().unwrap();
    drop(ui);
    drop(s);

    let s = open(&lib);
    assert!(s.notices().is_empty(), "{:?}", s.notices());
    assert_eq!(s.settings().save(), save);
    assert_eq!(s.settings().export(), export);
    assert_eq!((s.settings().export().jpeg_quality(), s.settings().export().long_side()), (Some(42), None));
    assert!(s.settings().edit().single_click_leaves, "the other settings untouched");
    assert_eq!(s.export_location(), Some(location.clone()));

    let file = stored(&lib);
    assert_eq!(file["future"], json!({"c": 1}));
    assert_eq!(file["save"]["futureFlag"], json!(true));
    assert_eq!(file["export"]["futurePreset"], json!("web"));
    assert_eq!(file["save"], json!({"futureFlag": true, "explain": false, "lastFolder": folder, "lastFormat": "png"}), "the documented keys");
    assert_eq!(
        file["export"],
        json!({"futurePreset": "web", "location": location, "resize": false, "size": 2048, "quality": "custom", "customQuality": 42,
               "numbers": true, "watermark": true, "watermarkText": "© Ana Costa 2026", "removeLocation": true})
    );

    // Back to the defaults: the folder and location are removed, the rest written as defaults.
    let mut st = s.settings().clone();
    st.set_save(&SaveSettings::default());
    st.set_export(&ExportSettings::default());
    assert_eq!(st.get("save").and_then(|v| v.get("lastFolder")), None);
    assert_eq!(st.get("export").and_then(|v| v.get("location")), None);
    assert_eq!((st.save(), st.export()), (SaveSettings::default(), ExportSettings::default()));
    assert_eq!(st.get("export").unwrap()["futurePreset"], json!("web"));
    // Every quality and format survives a write.
    for q in ExportQuality::ALL {
        st.set_export(&ExportSettings { quality: q, ..ExportSettings::default() });
        assert_eq!(Settings::parse(&st.print()).unwrap().export().quality, q);
        assert_eq!(ExportQuality::from_id(q.id()), Some(q));
    }
    assert_eq!(
        ExportQuality::ALL.map(ExportQuality::preset),
        [None, Some(85), Some(95), Some(60), None],
        "Automatic, Normal, Maximum, Minimum, Custom"
    );
    for f in [SaveFormat::Jpeg, SaveFormat::Png] {
        st.set_save(&SaveSettings { last_format: f, ..SaveSettings::default() });
        assert_eq!(Settings::parse(&st.print()).unwrap().save().last_format, f);
    }
}

#[test]
fn wrong_types_read_as_defaults_and_are_reported_by_key_never_by_value() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("library");
    write_settings(
        &lib,
        &json!({
            "save": {"explain": "yes", "lastFolder": "relative/secret-folder", "lastFormat": "gif"},
            "export": {
                "location": 7,
                "resize": 1,
                "size": 99_999,
                "quality": "best",
                "customQuality": 0,
                "numbers": "on",
                "watermark": null,
                "watermarkText": ["secret-watermark"],
                "removeLocation": "true",
            },
        }),
    );
    let s = open(&lib);
    assert_eq!(s.settings().save(), SaveSettings::default());
    assert_eq!(s.settings().export(), ExportSettings::default());
    let notes = s.notices().join("\n");
    for key in [
        "save.explain",
        "save.lastFolder",
        "save.lastFormat",
        "export.location",
        "export.resize",
        "export.size",
        "export.quality",
        "export.customQuality",
        "export.numbers",
        "export.watermark is",
        "export.watermarkText",
        "export.removeLocation",
    ] {
        assert!(notes.contains(key), "{key} in {notes}");
    }
    assert!(!notes.contains("secret"), "values never reach a notice: {notes}");
    assert_eq!(s.notices().len(), 12, "each once: {notes}");
    drop(s);

    let st = Settings::parse(br#"{"save": [], "export": "big"}"#).unwrap();
    assert_eq!((st.save(), st.export()), (SaveSettings::default(), ExportSettings::default()));
    assert_eq!(
        st.problems(),
        ["settings.json: \"save\" is not an object; its defaults are used", "settings.json: \"export\" is not an object; its defaults are used"]
    );
    // Hostile numbers at the edges.
    for (size, want) in [
        (json!(100), 100),
        (json!(4000), 4000),
        (json!(99), EXPORT_SIZE_DEFAULT),
        (json!(4001), EXPORT_SIZE_DEFAULT),
        (json!(1.5), 1600),
        (json!(-1), 1600),
        (json!(1e30), 1600),
    ] {
        let st = Settings::parse(serde_json::to_vec(&json!({"export": {"size": size}})).unwrap().as_slice()).unwrap();
        assert_eq!(st.export().size, want, "{size}");
    }
    for (q, want) in [(json!(1), 1), (json!(100), 100), (json!(101), CUSTOM_QUALITY_DEFAULT), (json!(256), 75), (json!("75"), 75)] {
        let st = Settings::parse(serde_json::to_vec(&json!({"export": {"customQuality": q}})).unwrap().as_slice()).unwrap();
        assert_eq!(st.export().custom_quality, want, "{q}");
    }
}

#[test]
fn values_out_of_range_are_never_stored() {
    let dir = tempfile::tempdir().unwrap();
    let mut st = Settings::default();
    let long_text = "é".repeat(MAX_WATERMARK_BYTES);
    st.set_export(&ExportSettings {
        location: Some(PathBuf::from("relative/folder")),
        size: *EXPORT_SIZE_RANGE.end() + 1,
        custom_quality: 0,
        watermark_text: long_text.clone(),
        ..ExportSettings::default()
    });
    let e = st.export();
    assert_eq!((e.location, e.size, e.custom_quality), (None, EXPORT_SIZE_DEFAULT, CUSTOM_QUALITY_DEFAULT));
    assert_eq!(e.watermark_text.len(), MAX_WATERMARK_BYTES, "cut on a character boundary");
    assert!(long_text.starts_with(&e.watermark_text));
    let long_path = dir.path().join("p".repeat(MAX_PATH_BYTES));
    st.set_save(&SaveSettings { last_folder: Some(long_path.clone()), ..SaveSettings::default() });
    assert_eq!(st.save().last_folder, None);
    st.set_export(&ExportSettings { location: Some(long_path), ..ExportSettings::default() });
    assert_eq!(st.export().location, None);
    assert!(st.problems().is_empty(), "{:?}", st.problems());
}
