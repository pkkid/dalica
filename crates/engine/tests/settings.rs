//! The settings in `settings.json` (`roadmap.md` M6.5, M7.3, M8.3, M9.3, M10.1; `data-model.md` §4
//! and §6, `menus-and-shortcuts.md` §5, `xmp-interop.md` §2, `editing.md` §1, `creating.md` §2,
//! `organizing.md` §4 and §9), each a row of [`rows`]: nothing stored means its default; every
//! setting round-trips through its documented key with unknown keys kept at every level; a value of
//! the wrong type is its default and is reported once by key (never by value) when the library
//! opens, the file left as the user wrote it. Then what is particular to some of them: tags and
//! crop ratios trimmed and capped, values out of range never stored, a section that is not an
//! object, the photo sizes by region, and the export location.

use std::path::{Path, PathBuf};

use dalica_engine::settings::{
    CUSTOM_QUALITY_DEFAULT, CropRatio, EXPORT_SIZE_DEFAULT, EXPORT_SIZE_RANGE, EditSettings, ExportQuality, ExportSettings, FILE, FontSize,
    InterfaceSettings, MAX_CROP_RATIOS, MAX_PATH_BYTES, MAX_RATIO_NAME, MAX_RECENT_TAGS, MAX_TAG_BYTES, MAX_WATERMARK_BYTES, MetadataWrites,
    OrganizeSettings, PhotoSizes, QuickTags, RECENT_DAYS_DEFAULT, RECENT_DAYS_RANGE, SMALL_PICTURE_DEFAULT, SaveFormat, SaveSettings, Settings,
    UiScaleSetting, default_export_location,
};
use dalica_fs::Expect;
use serde_json::{Value, json};

use crate::common::open;

/// One setting: its key (`section.key`), a value other than its default, a value of the wrong type
/// (holding "secret" where it is text, which no notice may show), its default, and how the session
/// reads it, as the JSON it is stored as.
struct Row {
    key: &'static str,
    sample: Value,
    wrong: Value,
    default: Value,
    read: fn(&Settings) -> Value,
}

fn row(key: &'static str, sample: Value, wrong: Value, default: Value, read: fn(&Settings) -> Value) -> Row {
    Row { key, sample, wrong, default, read }
}

fn ratios(list: &[CropRatio]) -> Value {
    list.iter().map(|r| json!({ "name": r.name, "width": r.width, "height": r.height })).collect()
}

/// Every setting; the folders are absolute paths under `dir`.
fn rows(dir: &Path) -> Vec<Row> {
    let quick = json!(["beach", null, null, "Grand-mère", null, null, null, null, null, "padded"]);
    vec![
        row("files.writeCaptionsTagsPlaces", json!(false), json!("no"), json!(true), |s| json!(s.metadata_writes().captions_tags_places)),
        row("files.writeStarsAsRatings", json!(true), json!(1), json!(false), |s| json!(s.metadata_writes().stars_as_ratings)),
        row("files.updateModificationTime", json!(true), json!(null), json!(false), |s| json!(s.metadata_writes().update_modification_time)),
        row("files.writeRawXmpSidecars", json!(true), json!([true]), json!(false), |s| json!(s.metadata_writes().raw_xmp_sidecars)),
        row("files.writeDates", json!(false), json!({ "on": false }), json!(true), |s| json!(s.metadata_writes().dates)),
        row("tags.quick", quick, json!("secret-tag"), Value::Array(vec![Value::Null; 10]), |s| json!(s.quick_tags().slots)),
        row("tags.useRecent", json!(false), json!("off"), json!(true), |s| json!(s.quick_tags().use_recent)),
        row("tags.recent", json!(["lisbon", "family"]), json!(5), json!([]), |s| json!(s.recent_tags())),
        row("edit.cropRatios", json!([{ "name": "Panorama", "width": 3.0, "height": 1.0 }]), json!({}), json!([]), |s| ratios(&s.edit().crop_ratios)),
        row("edit.lastCropRatio", json!("Panorama"), json!(12), json!(null), |s| json!(s.edit().last_crop_ratio)),
        row("edit.photoSizes", json!("us"), json!("imperial"), json!(null), |s| json!(s.edit().photo_sizes.map(PhotoSizes::id))),
        row("edit.singleClickLeaves", json!(true), json!("yes"), json!(false), |s| json!(s.edit().single_click_leaves)),
        row("save.explain", json!(false), json!("yes"), json!(true), |s| json!(s.save().explain)),
        row("save.lastFolder", json!(dir.join("Saved As")), json!("relative/secret-folder"), json!(null), |s| json!(s.save().last_folder)),
        row("save.lastFormat", json!("png"), json!("gif"), json!("jpeg"), |s| json!(s.save().last_format.id())),
        row("export.location", json!(dir.join("Exports")), json!(7), json!(null), |s| json!(s.export().location)),
        row("export.resize", json!(false), json!(1), json!(true), |s| json!(s.export().resize)),
        row("export.size", json!(2048), json!(99_999), json!(EXPORT_SIZE_DEFAULT), |s| json!(s.export().size)),
        row("export.quality", json!("custom"), json!("best"), json!("automatic"), |s| json!(s.export().quality.id())),
        row("export.customQuality", json!(42), json!(0), json!(CUSTOM_QUALITY_DEFAULT), |s| json!(s.export().custom_quality)),
        row("export.numbers", json!(true), json!("on"), json!(false), |s| json!(s.export().numbers)),
        row("export.watermark", json!(true), json!(null), json!(false), |s| json!(s.export().watermark)),
        row("export.watermarkText", json!("© Ana Costa 2026"), json!(["secret-watermark"]), json!(""), |s| json!(s.export().watermark_text)),
        row("export.removeLocation", json!(true), json!("true"), json!(false), |s| json!(s.export().remove_location)),
        row("organize.askBeforeMoving", json!(false), json!("no"), json!(true), |s| json!(s.organize().ask_before_moving)),
        row("organize.removeFromAlbumsWithoutAsking", json!(true), json!(1), json!(false), |s| json!(s.organize().remove_from_albums_without_asking)),
        row("organize.deleteWithoutAsking", json!(true), json!(null), json!(false), |s| json!(s.organize().delete_without_asking)),
        row("organize.recentDays", json!(90), json!("secret-days"), json!(RECENT_DAYS_DEFAULT), |s| json!(s.organize().recent_days)),
        row("organize.lastMoveFolder", json!(dir.join("Trips")), json!("relative/secret-folder"), json!(null), |s| {
            json!(s.organize().last_move_folder)
        }),
        row("interface.animations", json!(false), json!("no"), json!(true), |s| json!(s.interface().animations)),
        row("interface.tooltips", json!(false), json!(0), json!(true), |s| json!(s.interface().tooltips)),
        row("interface.systemTitleBar", json!(true), json!("on"), json!(false), |s| json!(s.interface().system_title_bar)),
        row("interface.uiScale", json!(150), json!(160), json!("auto"), |s| s.interface().ui_scale.to_json()),
        row("interface.fontSize", json!("large"), json!("huge"), json!("small"), |s| json!(s.interface().font_size.id())),
        row("interface.language", json!("pt-BR"), json!("secret language"), json!("system"), |s| json!(s.interface().language)),
        row("fileTypes.smallPictureThreshold", json!(128), json!(8), json!(SMALL_PICTURE_DEFAULT), |s| json!(s.small_picture_threshold())),
    ]
}

/// The sections of the rows.
const SECTIONS: [&str; 8] = ["files", "tags", "edit", "save", "export", "organize", "interface", "fileTypes"];

/// A `settings.json` holding `value(row)` for every row, an unknown key in every section and one at
/// the top.
fn file_of(rows: &[Row], value: impl Fn(&Row) -> Value) -> Value {
    let mut v = json!({ "future": { "a": [1, 2] } });
    for section in SECTIONS {
        v[section] = json!({ "futureKey": section });
    }
    for r in rows {
        let (section, key) = r.key.split_once('.').unwrap();
        v[section][key] = value(r);
    }
    v
}

fn write_settings(library: &Path, v: &Value) {
    dalica_fs::create_dir_all(library).unwrap();
    dalica_fs::replace(&library.join(FILE), serde_json::to_vec_pretty(v).unwrap().as_slice(), Expect::Absent).unwrap();
}

fn stored(library: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(library.join(FILE)).unwrap()).unwrap()
}

fn tags(list: &[&str]) -> Vec<String> {
    list.iter().map(|t| t.to_string()).collect()
}

fn ratio(name: &str, width: f64, height: f64) -> CropRatio {
    CropRatio { name: name.into(), width, height }
}

#[test]
fn nothing_stored_means_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(&dir.path().join("library"));
    for r in rows(dir.path()) {
        assert_eq!((r.read)(s.settings()), r.default, "{}", r.key);
    }
    assert_eq!(
        (s.settings().metadata_writes(), s.settings().quick_tags(), s.settings().edit()),
        (MetadataWrites::default(), QuickTags::default(), EditSettings::default())
    );
    assert_eq!((s.settings().save(), s.settings().export(), s.settings().organize()), Default::default());
    assert_eq!(s.settings().interface(), InterfaceSettings::default());
    assert_eq!(s.settings().edit().photo_sizes(), PhotoSizes::system());
    assert!(s.notices().is_empty(), "{:?}", s.notices());
}

/// Every setting written through its setter is stored under its documented key, reads back, and
/// leaves the unknown keys of every level where they were.
#[test]
fn every_setting_round_trips_with_unknown_keys_kept() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("library");
    let rows = rows(dir.path());
    write_settings(&lib, &file_of(&[], |_| Value::Null));
    let mut s = open(&lib);
    assert!(s.notices().is_empty(), "{:?}", s.notices());
    let wanted = Settings::parse(&serde_json::to_vec(&file_of(&rows, |r| r.sample.clone())).unwrap()).unwrap();
    assert!(wanted.problems().is_empty(), "{:?}", wanted.problems());
    let ui = dalica_fs::mark_ui_thread();
    s.change_settings(|st| {
        st.set_metadata_writes(&wanted.metadata_writes());
        st.set_quick_tags(&wanted.quick_tags());
        st.set_recent_tags(&wanted.recent_tags());
        st.set_edit(&wanted.edit());
        st.set_save(&wanted.save());
        st.set_export(&wanted.export());
        st.set_organize(&wanted.organize());
        st.set_interface(&wanted.interface());
        st.set_small_picture_threshold(wanted.small_picture_threshold());
    });
    s.flush().unwrap();
    drop(ui);
    drop(s);

    let s = open(&lib);
    assert!(s.notices().is_empty(), "{:?}", s.notices());
    let file = stored(&lib);
    for r in &rows {
        assert_eq!((r.read)(s.settings()), r.sample, "{} reads back", r.key);
        let (section, key) = r.key.split_once('.').unwrap();
        assert_eq!(file[section][key], r.sample, "{} is stored under its key", r.key);
    }
    assert_eq!(file["future"], json!({ "a": [1, 2] }));
    for section in SECTIONS {
        assert_eq!(file[section]["futureKey"], json!(section), "{section}: the unknown key is kept");
    }
}

#[test]
fn a_wrong_typed_value_is_its_default_and_is_reported_once_by_key_never_by_value() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("library");
    let rows = rows(dir.path());
    let written = file_of(&rows, |r| r.wrong.clone());
    write_settings(&lib, &written);
    let s = open(&lib);
    let notices = s.notices().join("\n");
    for r in &rows {
        assert_eq!((r.read)(s.settings()), r.default, "{}", r.key);
        let prefix = format!("settings.json: {} ", r.key);
        assert_eq!(s.notices().iter().filter(|n| n.starts_with(&prefix)).count(), 1, "{} once in:\n{notices}", r.key);
    }
    assert_eq!(s.notices().len(), rows.len(), "nothing else:\n{notices}");
    assert!(!notices.contains("secret"), "values never reach a notice:\n{notices}");
    // Nothing was rewritten: the file stays as the user left it until a setting changes.
    drop(s);
    assert_eq!(stored(&lib), written);
}

/// A section that is not an object: its defaults, reported once, replaced on the next write.
#[test]
fn a_section_that_is_not_an_object_is_reported_once_and_replaced_on_the_next_write() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("library");
    let mut written = json!({});
    let bad = [json!(true), json!(["beach"]), json!("nope"), json!([]), json!("big"), json!([true]), json!(3), json!("all")];
    for (section, bad) in SECTIONS.iter().zip(bad) {
        written[section] = bad;
    }
    write_settings(&lib, &written);
    let mut s = open(&lib);
    for r in rows(dir.path()) {
        assert_eq!((r.read)(s.settings()), r.default, "{}", r.key);
    }
    for section in SECTIONS {
        let said = format!("settings.json: \"{section}\" is not an object");
        assert_eq!(s.notices().iter().filter(|n| n.starts_with(&said)).count(), 1, "{section}: {:?}", s.notices());
    }
    s.change_settings(|st| {
        st.set_metadata_writes(&MetadataWrites { dates: false, ..MetadataWrites::default() });
        st.note_recent_tags(&tags(&["beach"]));
    });
    s.flush().unwrap();
    drop(s);
    let s = open(&lib);
    assert!(!s.notices().iter().any(|n| n.contains("\"files\"") || n.contains("\"tags\"")), "{:?}", s.notices());
    assert!(!s.settings().metadata_writes().dates);
    assert_eq!(s.settings().recent_tags(), tags(&["beach"]));
}

/// Quick tags are trimmed and blank slots unassigned; recent tags are most recent first, each
/// once, and capped; an overlong tag is never stored.
#[test]
fn tags_are_trimmed_capped_and_overlong_ones_never_stored() {
    let mut st = Settings::default();
    let mut quick = QuickTags { use_recent: false, ..QuickTags::default() };
    quick.slots[0] = Some("beach".into());
    quick.slots[5] = Some("   ".into());
    quick.slots[9] = Some("  padded  ".into());
    st.set_quick_tags(&quick);
    let back = st.quick_tags().slots;
    assert_eq!((back[0].as_deref(), back[5].as_deref(), back[9].as_deref()), (Some("beach"), None, Some("padded")));
    st.set_recent_tags(&tags(&["sunset", "family"]));
    st.note_recent_tags(&tags(&["lisbon", "family"]));
    assert_eq!(st.recent_tags(), tags(&["lisbon", "family", "sunset"]), "most recent first, each once");

    let many: Vec<String> = (0..25).map(|i| format!("t{i}")).collect();
    st.set_recent_tags(&many);
    assert_eq!(st.recent_tags(), many[..MAX_RECENT_TAGS].to_vec());
    st.note_recent_tags(&tags(&["new"]));
    assert_eq!(st.recent_tags().first().map(String::as_str), Some("new"));
    assert_eq!(st.recent_tags().len(), MAX_RECENT_TAGS, "the oldest fell off");
    assert_eq!(st.recent_tags().last().map(String::as_str), Some("t8"));
    let long = "x".repeat(MAX_TAG_BYTES + 1);
    let mut q = QuickTags::default();
    q.slots[0] = Some(long.clone());
    st.set_quick_tags(&q);
    st.note_recent_tags(std::slice::from_ref(&long));
    assert_eq!(st.get("tags").unwrap()["quick"][0], Value::Null);
    assert!(!st.recent_tags().contains(&long));
    assert!(st.problems().is_empty(), "what is written reads back without a problem");

    // Read from a file: what is usable is used, and the rest reported by count.
    let file = json!({ "tags": {
        "quick": ["beach", 7, null, { "t": "secret" }, "x".repeat(MAX_TAG_BYTES + 1), "", "a", "b", "c", "d", "eleventh"],
        "recent": ["sunset", false, "secret", "x".repeat(MAX_TAG_BYTES + 1), "sunset"],
    } });
    let st = Settings::parse(&serde_json::to_vec(&file).unwrap()).unwrap();
    assert_eq!(
        st.quick_tags().slots.iter().map(Option::as_deref).collect::<Vec<_>>(),
        [Some("beach"), None, None, None, None, None, Some("a"), Some("b"), Some("c"), Some("d")]
    );
    assert_eq!(st.recent_tags(), tags(&["sunset", "secret"]));
    let problems = st.problems().join("\n");
    for said in ["tags.quick: 3 slots", "more than 10 slots", "tags.recent: 2 entries"] {
        assert_eq!(problems.matches(said).count(), 1, "{said} in:\n{problems}");
    }
    assert!(!problems.contains("secret") && !problems.contains("beach"), "{problems}");
}

/// Crop ratios are trimmed, the first of a name kept, bad ones left out, and capped; going back to
/// the system's photo sizes removes the choice.
#[test]
fn crop_ratios_are_cleaned_and_capped() {
    let mut st = Settings::default();
    let want = EditSettings {
        crop_ratios: vec![
            ratio("Panorama", 3.0, 1.0),
            ratio("  Cinema  ", 2.39, 1.0),
            ratio("Cinema", 9.0, 9.0),
            ratio("", 1.0, 1.0),
            ratio("Bad", f64::NAN, 1.0),
        ],
        last_crop_ratio: Some("Cinema".into()),
        photo_sizes: Some(PhotoSizes::Us),
        single_click_leaves: true,
    };
    st.set_edit(&want);
    let back = Settings::parse(&st.print()).unwrap().edit();
    assert_eq!(back.crop_ratios, [ratio("Panorama", 3.0, 1.0), ratio("Cinema", 2.39, 1.0)], "trimmed, first of a name kept, bad ones left out");
    st.set_edit(&EditSettings { photo_sizes: None, last_crop_ratio: None, ..back });
    assert_eq!(st.get("edit").and_then(|e| e.get("photoSizes")), None);
    assert_eq!(st.get("edit").and_then(|e| e.get("lastCropRatio")), None);

    let many: Vec<CropRatio> = (0..(MAX_CROP_RATIOS + 20)).map(|i| ratio(&format!("r{i}"), 1.0 + i as f64, 1.0)).collect();
    let mut st = Settings::default();
    st.set_edit(&EditSettings { crop_ratios: many.clone(), last_crop_ratio: Some("x".repeat(MAX_RATIO_NAME + 1)), ..EditSettings::default() });
    let e = st.edit();
    assert_eq!(e.crop_ratios, many[..MAX_CROP_RATIOS]);
    assert_eq!(e.last_crop_ratio, None);
    st.set_edit(&EditSettings { crop_ratios: vec![ratio(&"n".repeat(MAX_RATIO_NAME + 1), 1.0, 1.0), ratio("big", 1e9, 1.0)], ..e });
    assert!(st.edit().crop_ratios.is_empty());
    // Read from a file: past the cap its first ratios, a bad one reported (never by name).
    let list: Vec<Value> = (0..(MAX_CROP_RATIOS + 1)).map(|i| json!({"name": format!("r{i}"), "width": 1, "height": 1})).collect();
    let st = Settings::parse(serde_json::to_vec(&json!({"edit": {"cropRatios": list}})).unwrap().as_slice()).unwrap();
    assert_eq!(st.edit().crop_ratios.len(), MAX_CROP_RATIOS);
    assert_eq!(st.problems().len(), 1);
    let file =
        json!({ "edit": { "cropRatios": [{"name": "secret-ratio-name", "width": -1, "height": 2}, {"name": "Ok", "width": 2, "height": 3}, 7] } });
    let st = Settings::parse(&serde_json::to_vec(&file).unwrap()).unwrap();
    assert_eq!(st.edit().crop_ratios, [ratio("Ok", 2.0, 3.0)]);
    assert!(st.problems().iter().any(|p| p.contains("edit.cropRatios: 2 entries")), "{:?}", st.problems());
    assert!(!st.problems().join("\n").contains("secret"));
}

/// Folders that are not absolute, overlong paths and text, and numbers out of range are never
/// stored; read from a file they are their defaults (the edges kept). Going back to the defaults
/// removes the folders and keeps the unknown keys.
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
    for days in [0, RECENT_DAYS_RANGE.end() + 1, u32::MAX] {
        st.set_organize(&OrganizeSettings { recent_days: days, last_move_folder: Some("relative".into()), ..OrganizeSettings::default() });
        let back = Settings::parse(&st.print()).unwrap();
        assert_eq!(back.organize().recent_days, RECENT_DAYS_DEFAULT, "{days}");
        assert_eq!(back.organize().last_move_folder, None);
        assert!(back.problems().is_empty(), "{:?}", back.problems());
    }
    for days in [*RECENT_DAYS_RANGE.start(), *RECENT_DAYS_RANGE.end()] {
        st.set_organize(&OrganizeSettings { recent_days: days, ..OrganizeSettings::default() });
        assert_eq!(Settings::parse(&st.print()).unwrap().organize().recent_days, days);
    }
    assert!(st.problems().is_empty(), "{:?}", st.problems());

    // Back to the defaults: the folders go, the unknown keys stay.
    let mut st = Settings::parse(br#"{"save": {"futureFlag": true}, "export": {"futurePreset": "web"}, "organize": {"futureKey": "kept"}}"#).unwrap();
    st.set_save(&SaveSettings { last_folder: Some(dir.path().join("Saved")), ..SaveSettings::default() });
    st.set_export(&ExportSettings { location: Some(dir.path().join("Out")), ..ExportSettings::default() });
    st.set_organize(&OrganizeSettings { last_move_folder: Some(dir.path().join("Trips")), ..OrganizeSettings::default() });
    st.set_save(&SaveSettings::default());
    st.set_export(&ExportSettings::default());
    st.set_organize(&OrganizeSettings::default());
    assert_eq!(st.get("save").and_then(|v| v.get("lastFolder")), None);
    assert_eq!(st.get("export").and_then(|v| v.get("location")), None);
    assert_eq!(st.get("organize").and_then(|v| v.get("lastMoveFolder")), None);
    assert_eq!((st.save(), st.export(), st.organize()), Default::default());
    assert_eq!((st.get("save").unwrap()["futureFlag"].clone(), st.get("export").unwrap()["futurePreset"].clone()), (json!(true), json!("web")));
    assert_eq!(st.get("organize").unwrap()["futureKey"], json!("kept"));

    // The UI scale, the language and the small-picture threshold out of range are their defaults.
    let mut st = Settings::default();
    let odd =
        InterfaceSettings { ui_scale: UiScaleSetting::Percent(130), language: "not a tag!".into(), font_size: FontSize::Large, ..Default::default() };
    st.set_interface(&odd);
    assert_eq!(st.interface(), InterfaceSettings { font_size: FontSize::Large, ..InterfaceSettings::default() });
    st.set_small_picture_threshold(4);
    assert_eq!(st.small_picture_threshold(), SMALL_PICTURE_DEFAULT);
    for (scale, want) in [
        (json!(75), UiScaleSetting::Percent(75)),
        (json!(300), UiScaleSetting::Percent(300)),
        (json!(50), UiScaleSetting::Auto),
        (json!(325), UiScaleSetting::Auto),
        (json!(110), UiScaleSetting::Auto),
        (json!(150.5), UiScaleSetting::Auto),
    ] {
        let st = Settings::parse(&serde_json::to_vec(&json!({ "interface": { "uiScale": scale } })).unwrap()).unwrap();
        assert_eq!(st.interface().ui_scale, want, "{scale}");
    }
    assert_eq!((UiScaleSetting::Percent(150).factor(), UiScaleSetting::Auto.factor()), (Some(1.5), None));
    for (px, want) in [(json!(16), 16), (json!(1024), 1024), (json!(15), 64), (json!(1025), 64), (json!(-64), 64)] {
        let st = Settings::parse(&serde_json::to_vec(&json!({ "fileTypes": { "smallPictureThreshold": px, "png": false } })).unwrap()).unwrap();
        assert_eq!(st.small_picture_threshold(), want, "{px}");
        assert!(!st.file_types().allows(dalica_scan::FileType::Png), "the type flags beside it still read");
    }

    // Read from a file: numbers at and past the edges.
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
    for days in [json!(0), json!(100_000), json!(2.5), json!(-3)] {
        let st = Settings::parse(&serde_json::to_vec(&json!({ "organize": { "recentDays": days } })).unwrap()).unwrap();
        assert_eq!(st.organize(), OrganizeSettings::default(), "{days}");
        assert_eq!(st.problems().len(), 1, "{days}: {:?}", st.problems());
        assert!(st.problems()[0].contains("organize.recentDays"), "{days}: {:?}", st.problems());
    }
}

/// Every export quality and Save As format survives a write, with its preset.
#[test]
fn every_quality_and_format_survives_a_write() {
    let mut st = Settings::default();
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
    let custom = ExportSettings { quality: ExportQuality::Custom, custom_quality: 42, resize: false, ..ExportSettings::default() };
    assert_eq!((custom.jpeg_quality(), custom.long_side()), (Some(42), None));
    let normal = ExportSettings::default();
    assert_eq!((normal.jpeg_quality(), normal.long_side()), (None, Some(1600)));
    for f in [SaveFormat::Jpeg, SaveFormat::Png] {
        st.set_save(&SaveSettings { last_format: f, ..SaveSettings::default() });
        assert_eq!(Settings::parse(&st.print()).unwrap().save().last_format, f);
    }
}

#[test]
fn the_photo_sizes_follow_the_region_until_chosen() {
    for (locale, sizes) in [
        (Some("en_US.UTF-8"), PhotoSizes::Us),
        (Some("en-US"), PhotoSizes::Us),
        (Some("es_US@euro"), PhotoSizes::Us),
        (Some("en_GB.UTF-8"), PhotoSizes::Metric),
        (Some("de_DE"), PhotoSizes::Metric),
        (Some("C"), PhotoSizes::Metric),
        (Some("POSIX"), PhotoSizes::Metric),
        (Some(""), PhotoSizes::Metric),
        (Some("en_USA"), PhotoSizes::Metric),
        (None, PhotoSizes::Metric),
    ] {
        assert_eq!(PhotoSizes::for_locale(locale), sizes, "{locale:?}");
    }
    let chosen = EditSettings { photo_sizes: Some(PhotoSizes::Metric), ..EditSettings::default() };
    assert_eq!(chosen.photo_sizes(), PhotoSizes::Metric);
    for p in [PhotoSizes::Us, PhotoSizes::Metric] {
        assert_eq!(PhotoSizes::from_id(p.id()), Some(p));
    }
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
