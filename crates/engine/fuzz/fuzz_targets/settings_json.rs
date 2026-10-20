//! Fuzz target `settings_json`: any bytes as the library's `settings.json` (`data-model.md` §4,
//! `security.md` §4.2, §7.1), read as a session reads it when it opens. Whatever the input: an
//! error with its reason (not JSON, not an object), and a session keeps the file aside as damaged
//! and uses the defaults; or settings, and then
//!
//! - every option of the Options catalogue reads a value its own check accepts unchanged (a bool,
//!   a whole number in its range and step, one of its choices, text within its length, an absolute
//!   folder or none; a stored language is any language tag, offered as the app would offer it),
//!   and the typed settings stay in their ranges (the window's size and position,
//!   the tree width, the thumbnail size, the scrolling thumb's curve);
//! - the settings print, parse back equal and print to the same bytes;
//! - storing every value in effect back (through each option's own `store`, and through each typed
//!   setter) reads back the same values, and storing them a second time changes nothing.
//!
//! Never a panic, a hang or an allocation far past the input.

#![no_main]

use dalica_engine::options::catalogue;
use dalica_engine::settings::{
    POSITION_LIMIT, Settings, THUMB_EXPONENT_RANGE, THUMB_SIZE_LIMIT, THUMB_TOP_SPEED_RANGE, TREE_WIDTH_LIMIT, WINDOW_MAX_SIDE, WINDOW_MIN,
    language_ok,
};
use libfuzzer_sys::fuzz_target;

/// Sets every typed group of `from` into `to` through its setter.
fn set_all(to: &mut Settings, from: &Settings) {
    if let Some(w) = from.window() {
        to.set_window(&w);
    }
    to.set_view(&from.view());
    to.set_view_options(&from.view_options());
    to.set_interface(&from.interface());
    to.set_small_picture_threshold(from.small_picture_threshold());
    to.set_thumb_speed(&from.thumb_speed());
    to.set_color_management(from.color_management());
    to.set_thumbnail_cache_bytes(from.thumbnail_cache_bytes());
    to.set_file_types(from.file_types());
    to.set_metadata_writes(&from.metadata_writes());
    to.set_quick_tags(&from.quick_tags());
    to.set_recent_tags(&from.recent_tags());
    to.set_edit(&from.edit());
    to.set_save(&from.save());
    to.set_export(&from.export());
    to.set_organize(&from.organize());
    to.set_slideshow(&from.slideshow());
    to.set_viewer(&from.viewer());
    to.set_import(&from.import());
    to.set_compare(&from.compare());
    to.set_people(&from.people());
    to.set_network(&from.network());
    to.set_legacy(&from.legacy());
    to.set_collage(&from.collage());
    to.set_external_editor(&from.external_editor());
    to.set_video(&from.video());
}

/// Every typed group `s` reads must equal what `t` reads.
fn same_typed(s: &Settings, t: &Settings) {
    assert_eq!(s.window(), t.window());
    assert_eq!(s.view(), t.view());
    assert_eq!(s.view_options(), t.view_options());
    assert_eq!(s.interface(), t.interface());
    assert_eq!(s.small_picture_threshold(), t.small_picture_threshold());
    assert_eq!(s.thumb_speed(), t.thumb_speed());
    assert_eq!(s.color_management(), t.color_management());
    assert_eq!(s.thumbnail_cache_bytes(), t.thumbnail_cache_bytes());
    assert_eq!(s.file_types(), t.file_types());
    assert_eq!(s.metadata_writes(), t.metadata_writes());
    assert_eq!(s.quick_tags(), t.quick_tags());
    assert_eq!(s.recent_tags(), t.recent_tags());
    assert_eq!(s.edit(), t.edit());
    assert_eq!(s.save(), t.save());
    assert_eq!(s.export(), t.export());
    assert_eq!(s.organize(), t.organize());
    assert_eq!(s.slideshow(), t.slideshow());
    assert_eq!(s.viewer(), t.viewer());
    assert_eq!(s.import(), t.import());
    assert_eq!(s.compare(), t.compare());
    assert_eq!(s.people(), t.people());
    assert_eq!(s.network(), t.network());
    assert_eq!(s.legacy(), t.legacy());
    assert_eq!(s.collage(), t.collage());
    assert_eq!(s.external_editor(), t.external_editor());
    assert_eq!(s.video(), t.video());
}

fuzz_target!(|data: &[u8]| {
    let mut s = match Settings::parse(data) {
        Ok(s) => s,
        Err(reason) => {
            assert!(!reason.trim().is_empty(), "a refusal without a reason");
            return;
        }
    };
    let _ = s.problems();
    let _ = s.keymap(&|_| true);
    // A stored language is any language tag, kept though this build may not offer it (a build with
    // more languages wrote it): the session offers the languages the app gives, here that one.
    let language = s.interface().language;
    assert!(language_ok(&language), "{language:?} is not a language tag");
    s.set_languages(&[(language.as_str(), "")]);

    // Every value in effect lies in its option's range.
    for spec in catalogue() {
        let value = (spec.read)(&s);
        match spec.check_with(&value, &s) {
            Ok(checked) => assert_eq!(checked, value, "{}: the value read is not the value it stores", spec.key),
            Err(e) => panic!("{}: the value read, {value}, is refused by its own check: {e}", spec.key),
        }
    }
    if let Some(w) = s.window() {
        assert!(w.size[0] >= WINDOW_MIN[0] && w.size[1] >= WINDOW_MIN[1] && w.size.iter().all(|v| *v <= WINDOW_MAX_SIDE), "{w:?}");
        assert!(w.position.is_none_or(|p| p.iter().all(|v| v.is_finite() && v.abs() <= POSITION_LIMIT)), "{w:?}");
    }
    assert!(s.view().tree_width.is_none_or(|w| w > 0.0 && w <= TREE_WIDTH_LIMIT));
    assert!(s.view_options().thumb_size.is_none_or(|t| t > 0.0 && t <= THUMB_SIZE_LIMIT));
    let speed = s.thumb_speed();
    assert!(THUMB_EXPONENT_RANGE.contains(&speed.exponent) && THUMB_TOP_SPEED_RANGE.contains(&speed.top_speed), "{speed:?}");

    // Print, parse, print.
    let printed = s.print();
    let back = Settings::parse(&printed).unwrap_or_else(|e| panic!("printed settings do not parse: {e}"));
    assert!(back == s, "printed settings parse back different");
    assert!(back.print() == printed, "printing the settings again changed them");

    // Every option's value stored back through its own `store`.
    let mut stored = s.clone();
    for spec in catalogue().iter().filter(|o| o.available()) {
        let value = (spec.read)(&s);
        spec.store(&mut stored, &value).unwrap_or_else(|e| panic!("{}: storing the value read fails: {e}", spec.key));
    }
    for spec in catalogue() {
        assert_eq!((spec.read)(&stored), (spec.read)(&s), "{}: stored back, it reads differently", spec.key);
    }
    let once = stored.print();
    for spec in catalogue().iter().filter(|o| o.available()) {
        let value = (spec.read)(&stored);
        spec.store(&mut stored, &value).unwrap_or_else(|e| panic!("{}: storing the value again fails: {e}", spec.key));
    }
    assert!(stored.print() == once, "storing every option a second time changed the file");

    // Every typed group stored back through its setter.
    let mut typed = s.clone();
    set_all(&mut typed, &s);
    same_typed(&typed, &s);
    let once = typed.print();
    let snapshot = typed.clone();
    set_all(&mut typed, &snapshot);
    assert!(typed.print() == once, "setting every group a second time changed the file");
});
