//! The section header block (M4.9, `ui-layout.md` §5.2), driven headlessly: every
//! `section:<id>/…` part is registered with its size at 1400 × 900 and 900 × 600; the buttons wait
//! for their commands, and Select Starred (live since M4.13) for a starred photo in its section;
//! the pinned form shows the first line only and owns the ids of what it covers; a right-click
//! opens the section menu with the items not yet there disabled.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_core::FolderId;
use dalica_engine::Session;
use dalica_ui::headless::Click;
use dalica_ui::panels::light_table::header::{Button, PLACEHOLDER, STICKY_ICON};
use dalica_ui::panels::light_table::layout::STICKY;
use dalica_ui::panels::light_table::{self, folder_name};
use dalica_ui::state::Section;
use dalica_ui::{App, Driver, DriverError};
use dalica_ui_kit::WidgetInfo;
use dalica_ui_kit::registry::Role;
use egui::{PointerButton, Rect, pos2, vec2};
use serde_json::json;

const FRAMES: usize = 60 * 60;
const SIZES: [(u32, u32); 2] = [(1400, 900), (900, 600)];

/// A session on a library under `dir` with `folders` folders sharing `photos` scenes photos.
fn session(dir: &Path, folders: usize, photos: usize) -> Session {
    let files = dalica_scenes::demo_library(folders, photos, 5).unwrap();
    let mut roots = BTreeSet::new();
    for f in &files {
        let path = dir.join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
        roots.insert(path.parent().unwrap().to_path_buf());
    }
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    for r in &roots {
        s.execute("folders.add", &json!({ "path": r })).unwrap();
    }
    s.wait_idle();
    s
}

fn info(d: &Driver) -> light_table::FrameInfo {
    d.app().panels().unwrap().light_table.frame_info().clone()
}

fn rect(w: &WidgetInfo) -> Rect {
    Rect::from_min_size(pos2(w.rect[0], w.rect[1]), vec2(w.rect[2], w.rect[3]))
}

fn get<'a>(widgets: &'a [WidgetInfo], id: &str) -> &'a WidgetInfo {
    widgets.iter().find(|w| w.id == id).unwrap_or_else(|| panic!("no {id}"))
}

fn folder_of(s: Section) -> FolderId {
    match s {
        Section::Folder(f) => f,
    }
}

/// Writes the last frame to `target/test-output/ui/<name>.png`, to look at.
fn save(d: &Driver, name: &str) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
    std::fs::create_dir_all(&dir).unwrap();
    let shot = d.screenshot().unwrap();
    std::fs::write(dir.join(format!("{name}.png")), dalica_scenes::fingerprint::png_rgba(shot.width, shot.height, &shot.rgba)).unwrap();
}

/// Checks the last frame: no interactive widget without an id, every id once.
fn ids_are_sound(d: &Driver, case: &str) {
    assert_eq!(d.app().panels().unwrap().unregistered_widgets(), [], "{case}: interactive widgets without an id");
    let widgets = d.widgets();
    let mut seen = BTreeSet::new();
    for w in &widgets {
        assert!(seen.insert(w.id.clone()), "{case}: `{}` registered twice", w.id);
    }
}

#[test]
fn every_part_is_there_with_its_size_at_both_window_sizes() {
    for (w, h) in SIZES {
        let dir = tempfile::tempdir().unwrap();
        let mut d = Driver::new(App::new(Some(session(dir.path(), 2, 12))), w, h, 1.0);
        d.settle(FRAMES).unwrap();
        // Star a photo of the first folder: Select Starred is live there and only there.
        let first = info(&d).headers[0].0;
        let photo = d.app().panels().unwrap().sections.get(first).unwrap().photos[0];
        d.run("photo.star.set", &json!({ "ids": [photo.get()], "on": true })).unwrap();
        d.settle(FRAMES).unwrap();
        save(&d, &format!("section-header-{w}x{h}"));
        ids_are_sound(&d, &format!("{w}×{h}"));

        let area = info(&d).area.unwrap();
        let widgets = d.widgets();
        let mut checked = 0;
        for (section, drawn) in info(&d).headers {
            if drawn.max.y > area.max.y {
                continue; // clipped by the view's bottom
            }
            let case = format!("{w}×{h} {section}");
            let id = format!("section:{section}");
            let block = get(&widgets, &id);
            assert_eq!(rect(block), Rect::from_min_size(drawn.min, vec2(area.width(), 96.0)), "{case}: full width × 96");
            assert!(block.enabled, "{case}: the header takes a right-click");
            assert_eq!(block.role, Role::Other);
            let part = |p: &str| get(&widgets, &format!("{id}/{p}")).clone();
            let inside = |p: &WidgetInfo| rect(block).contains_rect(rect(p));

            let icon = part("icon");
            assert_eq!(rect(&icon).size(), vec2(32.0, 32.0), "{case}: icon 32");
            assert_eq!(rect(&icon).min, drawn.min + vec2(12.0, 8.0), "{case}: 12 pt padding");
            let library = d.app().session().unwrap().library();
            let path = library.folder(folder_of(section)).unwrap().path.clone();
            assert_eq!(icon.label, path.display().to_string(), "{case}: the icon's tooltip is the full path");
            assert!(!icon.enabled, "{case}: Locate on Disk waits for the OS integration");

            let title = part("title");
            assert_eq!(title.label, folder_name(&path));
            assert_eq!(rect(&title).height(), 20.0, "{case}: the title row");
            assert!(rect(&title).min.x >= rect(&icon).max.x + 8.0, "{case}: the title is right of the icon");

            let description = part("description");
            assert_eq!(description.label, PLACEHOLDER);
            assert_eq!(rect(&description).height(), 18.0);
            assert!(description.enabled, "{case}: a click edits the description inline (M6.26)");

            let mut left = 0.0;
            for b in Button::ALL {
                let button = part(b.part());
                assert_eq!(rect(&button).size(), vec2(26.0, 22.0), "{case}: {b:?} is 26 × 22");
                assert!(rect(&button).min.x > left, "{case}: {b:?} in row order");
                left = rect(&button).max.x;
                assert!(rect(&button).min.y >= rect(&icon).max.y, "{case}: the button row is under the first line");
                assert!(rect(&button).max.y <= rect(&description).min.y, "{case}: and above the description");
                assert_eq!((button.role, button.label.as_str()), (Role::IconButton, b.tooltip()));
                if b == Button::SelectStarred {
                    assert_eq!(button.enabled, section == first, "{case}: Select Starred needs a starred photo in the section");
                } else {
                    assert!(!button.enabled, "{case}: {b:?} waits for {}", b.command());
                }
                if !button.enabled {
                    assert!(matches!(d.click(&format!("{id}/{}", b.part())), Err(DriverError::Disabled(_))), "{case}: {b:?}");
                }
            }
            for p in [&icon, &title, &description] {
                assert!(inside(p), "{case}: {} inside the header", p.id);
            }
            assert!(!widgets.iter().any(|w| w.id == format!("{id}/saveEdited")), "{case}: no unsaved edits, no Save at the right");
            checked += 1;
        }
        assert!(checked >= 1, "{w}×{h}: a whole header was on screen");

        // Select Starred selects the starred photo of its section.
        d.click(&format!("section:{first}/selectStarred")).unwrap();
        d.settle(FRAMES).unwrap();
        assert_eq!(d.state().selection.ids, [photo], "{w}×{h}");
        assert_eq!(d.state().selection.section, Some(first), "{w}×{h}");
    }
}

#[test]
fn a_pinned_header_shows_its_first_line_only_and_owns_its_ids() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Driver::new(App::new(Some(session(dir.path(), 3, 90))), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    let target = d.app().panels().unwrap().sections.list[1].id;
    d.run("section.scrollTo", &json!({ "section": target })).unwrap();
    d.settle(FRAMES).unwrap();
    let area = info(&d).area.unwrap();
    let id = format!("section:{target}");

    // Scrolled 40 into the section: the block is partly above the view, under the pinned form.
    // Scrolled 150: the block is gone; only the pinned line is left.
    let cell = d.widgets().into_iter().find(|w| w.id.starts_with("thumb:")).unwrap();
    for (by, block_parts) in [(40.0, vec!["description"]), (110.0, vec![])] {
        d.scroll(Some(&cell.id), vec2(0.0, -by)).unwrap();
        d.settle(FRAMES).unwrap();
        let case = format!("scrolled {}", d.state().scroll.offset);
        save(&d, &format!("section-header-pinned-{}", d.state().scroll.offset));
        ids_are_sound(&d, &case);
        let (pinned, band) = info(&d).sticky.expect("the header is pinned");
        assert_eq!((pinned, band), (target, Rect::from_min_size(area.min, vec2(area.width(), STICKY))), "{case}");
        let widgets = d.widgets();
        let header = get(&widgets, &id);
        assert_eq!(rect(header), band, "{case}: `{id}` is the pinned line, 32 high");
        let icon = get(&widgets, &format!("{id}/icon"));
        assert_eq!(rect(icon).size(), vec2(STICKY_ICON, STICKY_ICON), "{case}");
        let title = get(&widgets, &format!("{id}/title"));
        for p in [icon, title] {
            assert!(band.contains_rect(rect(p)), "{case}: {} on the pinned line", p.id);
        }
        // Nothing of the block's other rows is on the pinned line; what of the block shows below
        // it keeps its own ids.
        let parts: BTreeSet<&str> = widgets.iter().filter_map(|w| w.id.strip_prefix(&format!("{id}/"))).collect();
        let mut expected: BTreeSet<&str> = ["icon", "title"].into();
        expected.extend(block_parts.iter().copied());
        assert_eq!(parts, expected, "{case}");
        for p in &block_parts {
            assert!(rect(get(&widgets, &format!("{id}/{p}"))).min.y >= band.max.y, "{case}: {p} is below the pinned line");
        }
    }
}

#[test]
fn a_right_click_opens_the_section_menu_with_later_items_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Driver::new(App::new(Some(session(dir.path(), 2, 12))), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    let headers = info(&d).headers;
    let second = headers[1].0;
    assert_ne!(d.state().folder, Some(folder_of(second)));
    let id = format!("section:{second}");
    // On the header's free space, right of its parts.
    d.click_with(&id, &Click { fx: 0.8, fy: 0.5, button: PointerButton::Secondary, ..Click::default() }).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().folder, Some(folder_of(second)), "the right-clicked section is the selected one");
    let widgets = d.widgets();
    assert!(get(&widgets, &id).active, "its header shows the accent bar");
    let items: Vec<&WidgetInfo> = widgets.iter().filter(|w| w.id.starts_with("menuitem:")).collect();
    let ids: Vec<&str> = items.iter().map(|w| w.id.as_str()).collect();
    for expected in
        ["menuitem:section.properties", "menuitem:section.slideshow", "menuitem:folder.hide", "menuitem:folder.locate", "menuitem:folder.delete"]
    {
        assert!(ids.contains(&expected), "{expected} in {ids:?}");
    }
    let properties = items.iter().find(|w| w.id == "menuitem:section.properties").unwrap();
    assert!(properties.enabled, "Edit Description… opens Folder Properties for the right-clicked section");
    let remove = items.iter().find(|w| w.id == "menuitem:folder.remove").unwrap();
    assert!(remove.enabled, "Remove from Dalica… asks about the right-clicked section's folder");
    let live = ["menuitem:section.properties", "menuitem:folder.remove"];
    assert!(
        items.iter().filter(|w| !live.contains(&w.id.as_str())).all(|w| !w.enabled),
        "the items whose commands are not there yet are disabled: {ids:?}"
    );
    ids_are_sound(&d, "menu open");
    save(&d, "section-header-menu");
}
