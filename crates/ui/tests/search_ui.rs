//! Search mode in the window (M9.22, `organizing.md` §8, `ui-layout.md` §3 and §9), driven
//! headlessly on a demo library (three folders of twenty photos, 2026-06-04 to 06-06) whose clock
//! stands still on 2026-06-12, so the age filter reads the same everywhere. The search worker's
//! answer is waited for with `Session::wait_search` (no sleeps):
//!
//! - typing narrows the light table to the matching sections, and the banner says "Showing 20
//!   pictures in 1 folder (… s) · 212.2 KB" with the engine's numbers;
//! - each filter and the age combine with the text (the matches are the intersection), the age
//!   slider reading "Pictures up to 1 week old"; Faces is greyed with its reason;
//! - Show All leaves search mode and brings the scroll position back; Ctrl+F focuses the field,
//!   and Esc there does the same;
//! - a suggestion picked with ↓ and Enter puts `tag:<tag>` in the field and searches it;
//! - a search with no match shows the empty state;
//! - Show Duplicate Files shows the groups side by side under their banner;
//! - Save Search Results… opens Album Properties named for the search and OK makes the album;
//! - Search for This Tag in the Tags pane searches the tag;
//! - the toolbar while searching, the banner and the empty state match their fingerprints at
//!   1400 × 900 (`tests/fingerprints/region-toolbar-search-1400x900.json`,
//!   `region-banner-1400x900.json`, `region-no-results-1400x900.json`; `DALICA_BLESS=1` stores new
//!   ones, renders go to `target/test-output/ui/`).

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use crate::common::{FRAMES, LARGE, check_region, new_driver, shot, try_widget};
use dalica_core::{FolderId, PhotoId, Timestamp, VirtualClock};
use dalica_ui::headless::Click;
use dalica_ui::menus::{self, Lookup, Platform};
use dalica_ui::panels::{light_table, search_banner, toolbar};
use dalica_ui::state::{AlbumSection, DialogKind, Pane, ScrollAnchor, Section};
use dalica_ui::{App, Driver};
use dalica_ui_kit::widgets;
use egui::PointerButton;
use serde_json::json;

/// 2026-06-12 12:00 UTC: the demo's photos are six to eight days old.
const NOW_SECS: i64 = 1_781_265_600;

struct Fx {
    dir: tempfile::TempDir,
    d: Driver,
}

fn fixture((w, h): (u32, u32)) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(NOW_SECS * 1000)));
    let session = crate::common::demo_open(dir.path(), clock);
    let mut d = new_driver(App::new(Some(session)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    Fx { dir, d }
}

fn has(d: &Driver, id: &str) -> bool {
    try_widget(d, id).is_some()
}

/// The folder whose name ends with `name`.
fn folder(d: &Driver, name: &str) -> FolderId {
    let lib = d.app().session().unwrap().library();
    lib.folders().find(|f| f.path.to_string_lossy().ends_with(name)).map(|f| f.id).unwrap_or_else(|| panic!("no folder {name}"))
}

/// The photos of `folder`, in light-table order.
fn photos_in(d: &Driver, f: FolderId) -> Vec<PhotoId> {
    let lib = d.app().session().unwrap().library();
    let mut ids: Vec<PhotoId> = lib.photos().filter(|p| p.folder == f).map(|p| p.id).collect();
    ids.sort();
    ids
}

/// The light table's sections, top to bottom.
fn sections(d: &Driver) -> Vec<Section> {
    d.app().panels().unwrap().sections.list.iter().map(|s| s.id).collect()
}

/// Every photo the light table's folder sections show.
fn shown(d: &Driver) -> BTreeSet<PhotoId> {
    d.app().panels().unwrap().sections.list.iter().filter(|s| s.folder.is_some()).flat_map(|s| s.photos.iter().copied()).collect()
}

/// One frame (the view state's query goes to the engine), the worker's answer, then a settled
/// window.
fn answer(d: &mut Driver) {
    d.step();
    d.app_mut().session_mut().unwrap().wait_search();
    d.settle(FRAMES).unwrap();
}

/// The engine's matches as a set.
fn matches(d: &Driver) -> BTreeSet<PhotoId> {
    let s = d.app().session().unwrap().search_state();
    s.result.as_ref().map(|r| r.photos().iter().copied().collect()).unwrap_or_default()
}

/// Types `text` into the empty search field and waits for the answer.
fn search_for(d: &mut Driver, text: &str) {
    d.click("field:search").unwrap();
    d.type_text(text).unwrap();
    answer(d);
}

/// Leaves search mode with the banner's Show All.
fn show_all(d: &mut Driver) {
    d.click("button:search.showAll").unwrap();
    d.settle(FRAMES).unwrap();
}

/// The banner's text as registered (its label is the whole text).
fn banner(d: &Driver) -> String {
    try_widget(d, "banner:search").map(|w| w.label).unwrap_or_default()
}

/// Typing narrows the light table to the matching sections; the banner's text and counts are the
/// engine's; Show All brings everything and the scroll position back.
#[test]
fn typing_narrows_the_light_table_and_show_all_restores_it() {
    let mut fx = fixture((1400, 900));
    let d = &mut fx.d;
    let harbour = folder(d, "Harbour");
    let all = sections(d);
    assert_eq!(all.len(), 3, "{all:?}");
    assert!(!has(d, "banner:search") && !has(d, "region:banner"));
    // Scrolled to the last section before searching.
    d.run("section.scrollTo", &json!({ "section": Section::Folder(harbour) })).unwrap();
    d.settle(FRAMES).unwrap();
    let before = d.state().scroll;
    assert_eq!(before.section, Some(Section::Folder(harbour)));

    search_for(d, "harbour");
    assert_eq!(d.state().search.text, "harbour");
    assert_eq!(d.app().session().unwrap().search_state().text, "harbour", "the field's text went to the engine");
    assert_eq!(sections(d), [Section::Album(AlbumSection::Search), Section::Folder(harbour)], "only the matches show");
    assert_eq!(shown(d), photos_in(d, harbour).into_iter().collect::<BTreeSet<_>>());
    let r = d.app().session().unwrap().search_state().result.clone().unwrap();
    let text = banner(d);
    assert_eq!(text, search_banner::line(20, 1, r.seconds, r.bytes));
    assert!(text.starts_with("Showing 20 pictures in 1 folder (") && text.ends_with(" s) · 212.2 KB"), "{text}");
    assert!(has(d, "button:search.showAll") && has(d, "region:banner"));
    assert_eq!(d.state().search.saved_scroll, Some(before), "the scroll position is kept for later");
    assert_eq!(d.state().scroll, ScrollAnchor::default(), "the results start at the top");
    shot(d, "search-results-1400x900");

    d.click("button:search.showAll").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().search.text, "");
    assert!(!d.app().session().unwrap().search_state().active(), "the engine left search mode");
    assert!(!has(d, "banner:search"));
    assert_eq!(sections(d), all);
    assert_eq!((d.state().scroll, d.state().search.saved_scroll), (before, None), "the scroll position is back");

    // Ctrl+F puts the focus in the field; Esc there leaves search mode too.
    d.key("Ctrl+F").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(try_widget(d, "field:search").unwrap().active, "Ctrl+F focuses the search field");
    d.type_text("hills").unwrap();
    answer(d);
    assert!(has(d, "banner:search"));
    d.key("Escape").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().search.text, "");
    assert!(!has(d, "banner:search") && !d.app().session().unwrap().search_state().active());
    assert_eq!(d.state().scroll, before);
}

/// Each filter and the age combine with the text: the matches are what both find.
#[test]
fn filters_and_the_age_combine_with_the_text() {
    let mut fx = fixture((1400, 900));
    let d = &mut fx.d;
    let (harbour, hills) = (folder(d, "Harbour"), folder(d, "Hills"));
    let starred = [photos_in(d, harbour)[0], photos_in(d, harbour)[3], photos_in(d, hills)[1]];
    d.run("photo.star.set", &json!({ "ids": starred, "on": true })).unwrap();
    d.settle(FRAMES).unwrap();
    assert!(!try_widget(d, "button:filter.faces").unwrap().enabled, "Faces waits for faces");

    search_for(d, "img");
    let text_only = matches(d);
    assert_eq!(text_only.len(), 60);
    for id in ["button:filter.starred", "button:filter.videos", "button:filter.geotagged"] {
        // The filter alone…
        show_all(d);
        d.click(id).unwrap();
        answer(d);
        assert!(try_widget(d, id).unwrap().active, "{id} is on");
        let alone = matches(d);
        // …then with the text.
        d.click("field:search").unwrap();
        d.type_text("img").unwrap();
        answer(d);
        let both = matches(d);
        assert_eq!(both, alone.intersection(&text_only).copied().collect(), "{id}");
        assert_eq!(shown(d), both, "{id}: the light table shows them");
        d.click(id).unwrap();
        answer(d);
    }
    // Starred: three photos, two of them in Harbour.
    d.click("button:filter.starred").unwrap();
    answer(d);
    assert_eq!(matches(d), starred.iter().copied().collect());
    d.click("button:filter.starred").unwrap();
    answer(d);

    // The age: one week back from June 12 leaves the oldest folder out.
    show_all(d);
    d.app_mut().state_mut().search.age = widgets::age_position(Some(7));
    answer(d);
    assert_eq!(d.app().session().unwrap().search_state().age_days, Some(7), "the slider's position went to the engine in days");
    assert_eq!(try_widget(d, "slider:age").unwrap().label, "Pictures up to 1 week old");
    let recent = matches(d);
    assert!(!recent.is_empty() && recent.len() < 60, "{}", recent.len());
    assert!(recent.iter().all(|id| !photos_in(d, harbour).contains(id)), "Harbour is eight days old");
    assert!(has(d, "banner:search"), "an age alone is search mode");
    d.click("field:search").unwrap();
    d.type_text("hills").unwrap();
    answer(d);
    let hills_all: BTreeSet<PhotoId> = photos_in(d, hills).into_iter().collect();
    assert_eq!(matches(d), recent.intersection(&hills_all).copied().collect());
    // The hovered slider's label replaces the caption.
    d.hover("slider:age", 100).unwrap();
}

/// The autocomplete lists the tags for the last word; ↓ and Enter put `tag:<tag>` in the field.
#[test]
fn a_suggestion_picked_searches_the_tag() {
    suggestion_picked(LARGE);
}

fn suggestion_picked(size: (u32, u32)) {
    let mut fx = fixture(size);
    let d = &mut fx.d;
    let hills = photos_in(d, folder(d, "Hills"));
    d.run("photo.tags.add", &json!({ "ids": &hills[0..4], "tags": "tram" })).unwrap();
    d.settle(FRAMES).unwrap();
    search_for(d, "tra");
    shot(d, &format!("search-suggestions-{}x{}", size.0, size.1));
    let row = try_widget(d, &toolbar::option_id(0)).expect("a suggestion is listed");
    assert_eq!(row.label, "tram (4)");
    assert!(!row.active);
    d.key("Down").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().search.highlight, Some(0));
    assert!(try_widget(d, &toolbar::option_id(0)).unwrap().active, "the row is highlighted");
    d.key("Enter").unwrap();
    answer(d);
    assert_eq!(d.state().search.text, "tag:tram");
    assert_eq!(matches(d), hills[0..4].iter().copied().collect());
    assert!(!has(d, &toolbar::option_id(0)), "the list closed with the pick");
    // A click picks too, replacing only the last word.
    show_all(d);
    search_for(d, "img tr");
    d.click(&toolbar::option_id(0)).unwrap();
    answer(d);
    assert_eq!(d.state().search.text, "img tag:tram");
    assert_eq!(matches(d).len(), 4);
}

/// A search that matches nothing shows the empty state under the banner.
#[test]
fn no_match_shows_the_empty_state() {
    let mut fx = fixture((1400, 900));
    let d = &mut fx.d;
    search_for(d, "zzzz");
    assert!(sections(d).is_empty());
    let empty = try_widget(d, light_table::NO_MATCHES_ID).expect("the empty state");
    assert!(empty.label.contains(light_table::NO_MATCHES_TITLE), "{}", empty.label);
    assert!(banner(d).starts_with("Showing 0 pictures in 0 folders ("), "{}", banner(d));
    // Save Search Results… has nothing to save.
    let item = menu_item(d, "Save Search Results…");
    assert_eq!((item.enabled, item.reason.as_deref()), (false, Some("No photos match the search.")));
}

/// The bar's item under Tools ▸ Experimental labelled `label`.
fn menu_item(d: &Driver, label: &str) -> menus::ItemNode {
    let bar = menus::bar(d.app(), Platform::Other);
    let path = vec!["Tools".to_string(), "Experimental".to_string(), label.to_string()];
    menus::find(&bar, &Lookup::Path(path)).unwrap_or_else(|| panic!("no Tools ▸ Experimental ▸ {label}")).clone()
}

/// Copies two demo photos into a fourth folder: two duplicate groups of two files.
fn add_copies(fx: &mut Fx) -> Vec<PhotoId> {
    let photos = fx.dir.path().join("photos");
    let copies = photos.join("Copies");
    std::fs::create_dir_all(&copies).unwrap();
    let mut originals = Vec::new();
    for name in ["Harbour", "Garden"] {
        let f = folder(&fx.d, name);
        let id = photos_in(&fx.d, f)[2];
        let lib = fx.d.app().session().unwrap().library();
        let p = lib.photo(id).unwrap();
        let path = lib.folder(f).unwrap().path.join(&p.name);
        std::fs::copy(&path, copies.join(format!("{name}-{}", p.name))).unwrap();
        originals.push(id);
    }
    let s = fx.d.app_mut().session_mut().unwrap();
    s.execute("folders.add", &json!({ "path": copies })).unwrap();
    s.wait_idle();
    fx.d.settle(FRAMES).unwrap();
    originals
}

/// Show Duplicate Files: the groups side by side in their own section, under their banner; Show
/// All leaves.
#[test]
fn show_duplicate_files_shows_the_groups_and_their_banner() {
    let mut fx = fixture((1400, 900));
    let originals = add_copies(&mut fx);
    let d = &mut fx.d;
    let item = menu_item(d, "Show Duplicate Files");
    assert_eq!(item.shortcut.as_deref(), Some("Ctrl+F6"));
    let job = menus::activate(d.app_mut(), &item, &json!({})).unwrap()["job"].as_u64().unwrap();
    d.app_mut().session_mut().unwrap().wait_job(job, Duration::from_secs(60)).unwrap();
    d.settle(FRAMES).unwrap();
    assert!(d.state().search.duplicates, "the view state follows the engine's mode");
    assert_eq!(banner(d), "2 duplicate groups (4 files)");
    let first = d.app().panels().unwrap().sections.list[0].clone();
    assert_eq!((first.id, first.title.as_str()), (Section::Album(AlbumSection::Search), light_table::DUPLICATES_TITLE));
    assert_eq!(first.photos.len(), 4);
    // Each group's two files side by side.
    let groups = d.app().session().unwrap().search_state().result.clone().unwrap().groups;
    assert_eq!(first.photos, groups.concat());
    for g in &groups {
        assert_eq!(g.len(), 2);
        assert_eq!(g.iter().filter(|id| originals.contains(id)).count(), 1, "{g:?}: an original and its copy");
    }
    assert!(has(d, &format!("thumb:{}/in/search", first.photos[0])));
    d.click("button:search.showAll").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(!d.state().search.duplicates && !has(d, "banner:search"));
    assert!(!d.app().session().unwrap().search_state().active());
}

/// Save Search Results…: Album Properties named for the search; OK makes the album of the
/// matches, selected.
#[test]
fn save_search_results_makes_the_album() {
    let mut fx = fixture((1400, 900));
    let d = &mut fx.d;
    let harbour = photos_in(d, folder(d, "Harbour"));
    assert!(!menu_item(d, "Save Search Results…").enabled, "not searching");
    search_for(d, "harbour");
    let item = menu_item(d, "Save Search Results…");
    assert!(item.enabled, "{:?}", item.reason);
    assert_eq!(menus::activate(d.app_mut(), &item, &json!({})).unwrap(), json!({ "dialog": "albumProperties" }));
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().dialogs.last().map(|x| x.kind), Some(DialogKind::AlbumProperties));
    let draft = d.state().album_properties.clone().unwrap();
    assert_eq!((draft.album, draft.name.as_str(), draft.date.as_str()), (None, "Search results for \"harbour\"", "2026-06-04"));
    let undo = d.app().session().unwrap().undo_stack().len();
    d.click("button:dialog.ok").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(d.state().dialogs.is_empty());
    let lib = d.app().session().unwrap().library();
    let albums: Vec<_> = lib.albums().map(|(id, info)| (id.clone(), info.name.clone())).collect();
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].1, "Search results for \"harbour\"");
    let mut members = lib.album_members(&albums[0].0).to_vec();
    members.sort();
    assert_eq!(members, harbour);
    assert_eq!(d.app().session().unwrap().undo_stack().len(), undo + 1, "one undo step");
    assert_eq!(d.state().selection.section.and_then(Section::user_album), Some(albums[0].0.clone()), "the new album is selected");
}

/// Search for This Tag in the Tags pane's tag menu searches the tag.
#[test]
fn search_for_this_tag_searches_it() {
    let mut fx = fixture((1400, 900));
    let d = &mut fx.d;
    let garden = photos_in(d, folder(d, "Garden"));
    d.run("photo.tags.add", &json!({ "ids": &garden[5..8], "tags": "beach" })).unwrap();
    d.run("select.set", &json!({ "ids": &garden[5..6] })).unwrap();
    d.run("pane.tags", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().pane, Some(Pane::Tags));
    d.click_with("tag:beach", &Click { button: PointerButton::Secondary, ..Click::default() }).unwrap();
    d.settle(FRAMES).unwrap();
    d.click("menuitem:search.tag").unwrap();
    answer(d);
    assert_eq!(d.state().search.text, "tag:beach");
    assert_eq!(matches(d), garden[5..8].iter().copied().collect());
    assert_eq!(shown(d), garden[5..8].iter().copied().collect());
}

/// The toolbar while searching (text, Starred and an age set), the banner (Find Duplicates' text,
/// which holds no timing) and the empty state of a search with no match.
#[test]
fn the_toolbar_banner_and_empty_state_match_their_fingerprints() {
    let size = LARGE;
    let case = format!("{}x{}", size.0, size.1);
    let mut fx = fixture(size);
    add_copies(&mut fx);
    let d = &mut fx.d;
    let search = &mut d.app_mut().state_mut().search;
    search.text = "harbour".into();
    search.filters.starred = true;
    search.age = widgets::age_position(Some(91));
    answer(d);
    check_region(d, "region:toolbar", &format!("region-toolbar-search-{case}"));

    dalica_ui::commands::search::clear(d.app_mut());
    d.settle(FRAMES).unwrap();
    let job = d.run("search.duplicates", &json!({})).unwrap()["job"].as_u64().unwrap();
    d.app_mut().session_mut().unwrap().wait_job(job, Duration::from_secs(60)).unwrap();
    d.settle(FRAMES).unwrap();
    check_region(d, "region:banner", &format!("region-banner-{case}"));

    dalica_ui::commands::search::clear(d.app_mut());
    d.app_mut().state_mut().search.text = "zzzz".into();
    answer(d);
    check_region(d, "region:lightTable", &format!("region-no-results-{case}"));
}
