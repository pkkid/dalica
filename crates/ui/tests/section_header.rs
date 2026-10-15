//! The section header block (M4.9, `ui-layout.md` §5.2), driven headlessly: every
//! `section:<id>/…` part is registered with its size at 1400 × 900 and 900 × 600; the buttons wait
//! for their commands, Select Starred (live since M4.13) for a starred photo in its section and
//! Save (M8.19) for an unsaved edit in its folder;
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
use dalica_ui::state::{AlbumSection, Section};
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
    s.folder().unwrap()
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
        // Starred Photos now comes first, with the starred photo.
        let starred = Section::Album(AlbumSection::Starred);
        assert_eq!(info(&d).headers[0].0, starred, "{w}×{h}");
        assert_eq!(d.app().panels().unwrap().sections.get(starred).unwrap().photos, [photo]);
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
            let name = match section {
                Section::Folder(_) => {
                    let path = library.folder(folder_of(section)).unwrap().path.clone();
                    assert_eq!(icon.label, path.display().to_string(), "{case}: the icon's tooltip is the full path");
                    folder_name(&path)
                }
                Section::Album(_) => {
                    assert_eq!(icon.label, "Starred Photos", "{case}: an album's icon shows its name");
                    "Starred Photos".to_string()
                }
            };
            assert!(!icon.enabled, "{case}: Locate on Disk waits for the OS integration; an album has no file");

            let title = part("title");
            assert_eq!(title.label, name);
            assert_eq!(rect(&title).height(), 20.0, "{case}: the title row");
            assert!(rect(&title).min.x >= rect(&icon).max.x + 8.0, "{case}: the title is right of the icon");

            if section == starred {
                // The automatic albums have no description; the button row is the block's last.
                assert!(!widgets.iter().any(|w| w.id == format!("{id}/description")), "{case}: no description");
                for b in Button::ALL {
                    let button = part(b.part());
                    assert_eq!(rect(&button).size(), vec2(26.0, 22.0), "{case}: {b:?} is 26 × 22");
                    assert!(inside(&button), "{case}: {b:?} inside the header");
                }
                assert!(part(Button::SelectStarred.part()).enabled, "{case}: its photos are starred");
                assert!(part(Button::Export.part()).enabled, "{case}: Export takes the album's photos");
                for p in [&icon, &title] {
                    assert!(inside(p), "{case}: {} inside the header", p.id);
                }
                checked += 1;
                continue;
            }
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
                } else if b == Button::Export {
                    assert!(button.enabled, "{case}: Export opens the Export dialog (output.export, M8.4)");
                } else if b == Button::Save {
                    assert!(!button.enabled, "{case}: Save needs an unsaved edit in the folder (M8.19, `save_ui.rs`)");
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

/// With unsaved edits the first line ends with the Save text button, in the pinned form too, which
/// then owns `/saveEdited`; a click on it asks the first-time explanation of Save for the folder's
/// edited photos (M8.19).
#[test]
fn a_pinned_header_keeps_its_save_button() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Driver::new(App::new(Some(session(dir.path(), 3, 90))), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    let target = d.app().panels().unwrap().sections.list[1].id;
    let photos = d.app().panels().unwrap().sections.get(target).unwrap().photos.clone();
    for p in &photos[..2] {
        d.run("photo.history.apply", &json!({ "id": p, "op": "autofix()" })).unwrap();
    }
    d.run("section.scrollTo", &json!({ "section": target })).unwrap();
    d.settle(FRAMES).unwrap();
    let id = format!("section:{target}/saveEdited");
    assert!(get(&d.widgets(), &id).enabled, "the block's Save");
    let cell = d.widgets().into_iter().find(|w| w.id.starts_with("thumb:")).unwrap();
    d.scroll(Some(&cell.id), vec2(0.0, -110.0)).unwrap();
    d.settle(FRAMES).unwrap();
    ids_are_sound(&d, "pinned with edits");
    let (pinned, band) = info(&d).sticky.expect("the header is pinned");
    assert_eq!(pinned, target);
    let button = get(&d.widgets(), &id).clone();
    assert!(button.enabled && band.contains_rect(rect(&button)), "the pinned line's Save: {button:?}");
    assert_eq!(d.app().panels().unwrap().light_table.headers.edited(target), Some(2));
    save(&d, "section-header-pinned-edited");
    d.click(&id).unwrap();
    d.settle(FRAMES).unwrap();
    let dialog = d.state().dialogs.last().expect("the explanation");
    assert_eq!(dialog.kind.name(), "saveExplain");
    let mut asked: Vec<u64> = serde_json::from_value(dialog.command.as_ref().unwrap().params["ids"].clone()).unwrap();
    asked.sort_unstable();
    let mut edited: Vec<u64> = photos[..2].iter().map(|p| p.get()).collect();
    edited.sort_unstable();
    assert_eq!(asked, edited, "Save names the folder's edited photos");
}

/// Checks the rect `r` of the last frame against `tests/fingerprints/<name>.json` (`DALICA_BLESS=1`
/// stores a new one; the region goes to the test output, `-actual` when it differs).
fn check_rect(d: &Driver, r: Rect, name: &str) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
    std::fs::create_dir_all(&dir).unwrap();
    let shot = d.screenshot().unwrap();
    let (x0, y0, w, h) = (r.min.x.round() as u32, r.min.y.round() as u32, r.width().round() as u32, r.height().round() as u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            rgba.extend_from_slice(&shot.pixel(x, y).unwrap());
        }
    }
    use dalica_scenes::fingerprint;
    std::fs::write(dir.join(format!("{name}.png")), fingerprint::png_rgba(w, h, &rgba)).unwrap();
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{name}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(w, h, &rgba)) + "\n").unwrap();
    }
    let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap()).unwrap();
    if let Err(m) = fingerprint::check(w, h, &rgba, &expected, 2.0) {
        std::fs::write(dir.join(format!("{name}-actual.png")), &m.actual_png).unwrap();
        panic!("{name}: {}", m.message);
    }
}

/// An album's header (M9.17): the album's icon, name, date and place, the buttons as for folders,
/// and the description edited inline through `album.info.set` (one undo step); a right-click
/// selects the album, so the section menu is the Album variant; selected, it has the accent bar.
#[test]
fn an_album_header_shows_the_album_and_edits_its_description() {
    for (w, h) in SIZES {
        let case = format!("{w}×{h}");
        let dir = tempfile::tempdir().unwrap();
        let mut d = Driver::new(App::new(Some(session(dir.path(), 2, 12))), w, h, 1.0);
        d.settle(FRAMES).unwrap();
        let folder = d.app().panels().unwrap().sections.list[0].id;
        let photos = d.app().panels().unwrap().sections.get(folder).unwrap().photos.clone();
        let made = d
            .run("album.create", &json!({ "name": "Best of the Harbour", "date": "2026-06-04", "place": "Lisbon", "ids": [photos[0], photos[1]] }))
            .unwrap();
        let album = dalica_core::AlbumId::parse(made["album"].as_str().unwrap()).unwrap();
        let section = Section::Album(AlbumSection::User(dalica_ui::state::AlbumKey::of(&album)));
        d.settle(FRAMES).unwrap();
        assert_eq!(info(&d).headers[0].0, section, "{case}: the album's section comes first");
        let id = format!("section:{section}");
        let widgets = d.widgets();
        assert_eq!(get(&widgets, &format!("{id}/icon")).label, "Best of the Harbour", "{case}: the icon shows the name");
        assert_eq!(get(&widgets, &format!("{id}/title")).label, "Best of the Harbour");
        assert_eq!(get(&widgets, &format!("{id}/description")).label, PLACEHOLDER);
        for b in Button::ALL {
            assert_eq!(rect(get(&widgets, &format!("{id}/{}", b.part()))).size(), vec2(26.0, 22.0), "{case}: {b:?}");
        }
        assert!(get(&widgets, &format!("{id}/export")).enabled, "{case}: Export takes the album's photos");
        let headers = &d.app().panels().unwrap().light_table.headers;
        assert_eq!(headers.shown(section), Some(("Thursday, June 4, 2026 · Lisbon", "")), "{case}: the album's date and place");
        ids_are_sound(&d, &case);

        // A right-click selects the album: the section menu is the Album variant.
        d.click_with(&id, &Click { button: PointerButton::Secondary, ..Click::default() }).unwrap();
        d.settle(FRAMES).unwrap();
        assert_eq!(d.state().selection.section, Some(section), "{case}");
        assert!(d.widgets().iter().any(|w| w.id == "menuitem:album.delete"), "{case}: the Album rows");
        d.key("Escape").unwrap();
        d.settle(FRAMES).unwrap();
        let block = info(&d).headers.iter().find(|(s, _)| *s == section).unwrap().1;
        check_rect(&d, Rect::from_min_size(block.min, vec2(block.width(), 96.0)), &format!("region-header-album-{w}x{h}"));

        // The description, inline: Enter commits it to the album in one undo step.
        let steps = d.app().session().unwrap().undo_stack().len();
        d.click(&format!("{id}/description")).unwrap();
        d.settle(FRAMES).unwrap();
        d.type_text("Boats at dawn").unwrap();
        d.key("Enter").unwrap();
        d.settle(FRAMES).unwrap();
        let library = d.app().session().unwrap().library();
        assert_eq!(library.album(&album).unwrap().description, "Boats at dawn", "{case}");
        assert_eq!(d.app().session().unwrap().undo_stack().len(), steps + 1, "{case}: one undo step");
        assert_eq!(get(&d.widgets(), &format!("{id}/description")).label, "Boats at dawn");
        d.run("edit.undo", &json!({})).unwrap();
        d.settle(FRAMES).unwrap();
        assert_eq!(d.app().session().unwrap().library().album(&album).unwrap().description, "", "{case}: undone");
    }
}
