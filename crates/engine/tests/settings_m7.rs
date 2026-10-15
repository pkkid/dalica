//! M7's settings (`roadmap.md` M7.3): the Crop panel's custom ratios and last ratio, the photo sizes
//! it lists, and "Single-click to leave the edit view" round-trip through `settings.json` with
//! unknown keys kept at every level; a value of the wrong type is its default and is reported by
//! key when the library opens; the photo sizes follow the system's region until chosen.

use std::path::Path;

use crate::common::open;
use dalica_engine::settings::{CropRatio, EditSettings, FILE, MAX_CROP_RATIOS, MAX_RATIO_NAME, PhotoSizes, Settings};
use dalica_fs::Expect;
use serde_json::{Value, json};

fn write_settings(library: &Path, v: &Value) {
    dalica_fs::create_dir_all(library).unwrap();
    dalica_fs::replace(&library.join(FILE), serde_json::to_vec_pretty(v).unwrap().as_slice(), Expect::Absent).unwrap();
}

fn stored(library: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(library.join(FILE)).unwrap()).unwrap()
}

fn ratio(name: &str, width: f64, height: f64) -> CropRatio {
    CropRatio { name: name.into(), width, height }
}

#[test]
fn nothing_stored_means_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    let e = s.settings().edit();
    assert_eq!(e, EditSettings::default());
    assert!(e.crop_ratios.is_empty() && e.last_crop_ratio.is_none() && e.photo_sizes.is_none());
    assert!(!e.single_click_leaves, "editing.md §1: off");
    assert_eq!(e.photo_sizes(), PhotoSizes::system());
    assert!(s.notices().is_empty(), "{:?}", s.notices());
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
fn every_new_setting_round_trips_with_unknown_keys_kept() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path();
    write_settings(
        lib,
        &json!({
            "future": {"b": true},
            "edit": {"futureFlag": 3, "cropRatios": [{"name": "Panorama", "width": 3, "height": 1, "color": "red"}]},
            "tags": {"useRecent": false},
        }),
    );
    let mut s = open(lib);
    assert!(s.notices().is_empty(), "{:?}", s.notices());
    assert_eq!(s.settings().edit().crop_ratios, [ratio("Panorama", 3.0, 1.0)]);
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
    s.change_settings(|st| st.set_edit(&want));
    s.flush().unwrap();
    drop(s);

    let s = open(lib);
    assert!(s.notices().is_empty(), "{:?}", s.notices());
    let back = s.settings().edit();
    assert_eq!(back.crop_ratios, [ratio("Panorama", 3.0, 1.0), ratio("Cinema", 2.39, 1.0)], "trimmed, first of a name kept, bad ones left out");
    assert_eq!(back.last_crop_ratio.as_deref(), Some("Cinema"));
    assert_eq!(back.photo_sizes, Some(PhotoSizes::Us));
    assert!(back.single_click_leaves);
    assert!(!s.settings().quick_tags().use_recent, "the other settings untouched");

    let file = stored(lib);
    assert_eq!(file["future"], json!({"b": true}));
    assert_eq!(file["edit"]["futureFlag"], json!(3));
    assert_eq!(file["edit"]["cropRatios"][0]["color"], json!("red"), "a kept ratio keeps its unknown keys");
    assert_eq!(file["edit"]["photoSizes"], json!("us"));
    assert_eq!(file["edit"]["singleClickLeaves"], json!(true));

    // Going back to the system's sizes removes the choice.
    let mut st = s.settings().clone();
    st.set_edit(&EditSettings { photo_sizes: None, last_crop_ratio: None, ..back });
    assert_eq!(st.get("edit").and_then(|e| e.get("photoSizes")), None);
    assert_eq!(st.get("edit").and_then(|e| e.get("lastCropRatio")), None);
}

#[test]
fn wrong_types_read_as_defaults_and_are_reported_by_key() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path();
    write_settings(
        lib,
        &json!({
            "edit": {
                "cropRatios": [{"name": "secret-ratio-name", "width": -1, "height": 2}, {"name": "Ok", "width": 2, "height": 3}, 7],
                "lastCropRatio": 12,
                "photoSizes": "imperial",
                "singleClickLeaves": "yes",
            },
        }),
    );
    let s = open(lib);
    let e = s.settings().edit();
    assert_eq!(e.crop_ratios, [ratio("Ok", 2.0, 3.0)]);
    assert_eq!((e.last_crop_ratio, e.photo_sizes, e.single_click_leaves), (None, None, false));
    let notes = s.notices().join("\n");
    for key in ["edit.cropRatios: 2 entries", "edit.lastCropRatio", "edit.photoSizes", "edit.singleClickLeaves"] {
        assert!(notes.contains(key), "{key} in {notes}");
    }
    assert!(!notes.contains("secret-ratio-name"), "names never reach a notice");
    drop(s);

    let mut st = Settings::parse(br#"{"edit": "nope"}"#).unwrap();
    assert_eq!(st.edit(), EditSettings::default());
    assert_eq!(st.problems(), ["settings.json: \"edit\" is not an object; its defaults are used"]);
    st = Settings::parse(br#"{"edit": {"cropRatios": {}}}"#).unwrap();
    assert_eq!(st.problems(), ["settings.json: edit.cropRatios is not a list; no custom ratios are used"]);
}

#[test]
fn ratios_are_capped_and_overlong_names_never_stored() {
    let many: Vec<CropRatio> = (0..(MAX_CROP_RATIOS + 20)).map(|i| ratio(&format!("r{i}"), 1.0 + i as f64, 1.0)).collect();
    let mut st = Settings::default();
    st.set_edit(&EditSettings { crop_ratios: many.clone(), last_crop_ratio: Some("x".repeat(MAX_RATIO_NAME + 1)), ..EditSettings::default() });
    let e = st.edit();
    assert_eq!(e.crop_ratios, many[..MAX_CROP_RATIOS]);
    assert_eq!(e.last_crop_ratio, None);
    st.set_edit(&EditSettings { crop_ratios: vec![ratio(&"n".repeat(MAX_RATIO_NAME + 1), 1.0, 1.0), ratio("big", 1e9, 1.0)], ..e });
    assert!(st.edit().crop_ratios.is_empty());
    // A stored list past the cap reads its first ratios and says so.
    let list: Vec<Value> = (0..(MAX_CROP_RATIOS + 1)).map(|i| json!({"name": format!("r{i}"), "width": 1, "height": 1})).collect();
    let st = Settings::parse(serde_json::to_vec(&json!({"edit": {"cropRatios": list}})).unwrap().as_slice()).unwrap();
    assert_eq!(st.edit().crop_ratios.len(), MAX_CROP_RATIOS);
    assert_eq!(st.problems().len(), 1);
}
