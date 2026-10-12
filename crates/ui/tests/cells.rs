//! Thumbnail cells (M4.10, `ui-layout.md` §5.3–§5.5, `design-system.md` §7), driven headlessly on
//! the virtual clock: each caption mode's text (middle-elided in small cells), small pictures left
//! out and shown with View ▸ Small Pictures, the hidden-photo opacity rule, the badges fed from the
//! model (star and held), the "!" tile of a photo whose file is gone with its reason, and the hover
//! tooltip after 600 ms of virtual time and not before. Renders go to `target/test-output/ui/`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_core::PhotoId;
use dalica_engine::Session;
use dalica_library::Photo;
use dalica_ui::panels::light_table::cell::{self, Cells, HIDDEN_OPACITY, Look, Shown, TOOLTIP_SECONDS};
use dalica_ui::panels::thumbnails::Drawn;
use dalica_ui::state::{Caption, THUMB_MIN};
use dalica_ui::{App, Driver};
use dalica_ui_kit::Tokens;
use dalica_ui_kit::widgets::{Badge, Badges};
use egui::{Rect, pos2, vec2};
use serde_json::json;

const FRAMES: usize = 60 * 60;

/// Writes `n` demo photos (and `tiny` 32 × 32 PNGs) into one folder under `dir`; returns it.
fn photos(dir: &Path, n: usize, tiny: usize) -> PathBuf {
    let files = dalica_scenes::demo_library(1, n, 7).unwrap();
    let folder = dir.join("photos").join(files[0].path.parent().unwrap());
    std::fs::create_dir_all(&folder).unwrap();
    for f in &files {
        std::fs::write(dir.join("photos").join(&f.path), &f.bytes).unwrap();
    }
    for i in 0..tiny {
        let png = dalica_scenes::png(&dalica_scenes::Scene::new(900 + i as u64, 32, 32)).unwrap();
        std::fs::write(folder.join(format!("icon_{i}.png")), png).unwrap();
    }
    folder
}

fn session(dir: &Path, folder: &Path) -> Session {
    let mut s = Session::open(&dir.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    s
}

fn driver(session: Session, size: (u32, u32)) -> Driver {
    let mut d = Driver::new(App::new(Some(session)), size.0, size.1, 1.0);
    d.settle(FRAMES).unwrap();
    d
}

fn cells(d: &Driver) -> &Cells {
    &d.app().panels().unwrap().light_table.cells
}

fn shown(d: &Driver) -> Vec<Shown> {
    cells(d).shown().to_vec()
}

fn photo(d: &Driver, id: PhotoId) -> Photo {
    d.app().session().unwrap().library().photo(id).unwrap().clone()
}

/// The thumbnails' ids in the widget list (their parts left out).
fn thumbs(d: &Driver) -> Vec<String> {
    d.widgets().into_iter().map(|w| w.id).filter(|id| id.starts_with("thumb:") && !id.contains('/')).collect()
}

fn save(d: &Driver, name: &str) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
    std::fs::create_dir_all(&dir).unwrap();
    let shot = d.screenshot().unwrap();
    std::fs::write(dir.join(format!("{name}.png")), dalica_scenes::fingerprint::png_rgba(shot.width, shot.height, &shot.rgba)).unwrap();
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

#[test]
fn each_caption_mode_shows_its_text_middle_elided() {
    let dir = tempfile::tempdir().unwrap();
    let folder = photos(dir.path(), 6, 0);
    let mut d = driver(session(dir.path(), &folder), (1400, 900));
    assert!(shown(&d).iter().all(|s| s.caption.is_none()), "None is the default: no caption line");
    for (mode, name) in [
        (Caption::Filename, "filename"),
        (Caption::Date, "date"),
        (Caption::Resolution, "resolution"),
        (Caption::Caption, "caption"),
        (Caption::Tags, "tags"),
        (Caption::None, "none"),
    ] {
        d.run("view.caption", &json!({ "caption": name })).unwrap();
        d.settle(FRAMES).unwrap();
        assert_eq!(d.state().caption, mode);
        let all = shown(&d);
        assert_eq!(all.len(), 6, "{name}");
        for s in &all {
            let p = photo(&d, s.photo);
            let want = match mode {
                Caption::None => None,
                Caption::Filename => Some(p.name.clone()),
                Caption::Date => {
                    let date = p.date.or_else(|| p.sort_date()).unwrap().date();
                    Some(format!("{} {}, {}", MONTHS[usize::from(date.month()) - 1], date.day(), date.year()))
                }
                Caption::Resolution => Some(format!("{} × {}", p.width, p.height)),
                // Captions and tags arrive with M6: an empty line until then.
                Caption::Caption | Caption::Tags => Some(String::new()),
            };
            assert_eq!(s.caption, want, "{name}: {}", p.name);
        }
        if mode == Caption::Filename {
            save(&d, "cells-caption-filename-1400x900");
        }
    }
    // In the smallest cells a file name is cut in the middle, keeping both ends.
    d.run("view.caption", &json!({ "caption": "filename" })).unwrap();
    d.app_mut().state_mut().thumb_size = THUMB_MIN;
    d.settle(FRAMES).unwrap();
    save(&d, "cells-caption-small-1400x900");
    for s in shown(&d) {
        let name = photo(&d, s.photo).name;
        let text = s.caption.unwrap();
        assert!(text.contains('…') && text.chars().count() < name.chars().count(), "{name} → {text}");
        let (head, tail) = text.split_once('…').unwrap();
        assert!(name.starts_with(head) && name.ends_with(tail) && !head.is_empty() && !tail.is_empty(), "{name} → {text}");
    }
}

#[test]
fn small_pictures_are_hidden_and_shown() {
    let dir = tempfile::tempdir().unwrap();
    let folder = photos(dir.path(), 4, 3);
    let mut d = driver(session(dir.path(), &folder), (1400, 900));
    let library = d.app().session().unwrap().library();
    assert_eq!(library.photos().count(), 7, "the scan found the small pictures too");
    assert_eq!(thumbs(&d).len(), 4, "small pictures are left out by default");
    assert!(shown(&d).iter().all(|s| photo(&d, s.photo).width > 64));
    d.run("view.smallPictures", &json!({ "on": true })).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(thumbs(&d).len(), 7, "View ▸ Small Pictures shows them");
    let small: Vec<Shown> = shown(&d).into_iter().filter(|s| photo(&d, s.photo).width == 32).collect();
    assert_eq!(small.len(), 3);
    save(&d, "cells-small-pictures-1400x900");
    d.run("view.smallPictures", &json!({ "on": false })).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(thumbs(&d).len(), 4, "and hides them again");
}

#[test]
fn hidden_photos_are_drawn_at_40_percent_only_with_hidden_pictures() {
    // The rule.
    assert_eq!(cell::opacity(false, false), Some(1.0));
    assert_eq!(cell::opacity(false, true), Some(1.0));
    assert_eq!(cell::opacity(true, true), Some(HIDDEN_OPACITY));
    assert_eq!(cell::opacity(true, false), None, "a hidden photo is not drawn without Hidden Pictures");
    assert_eq!(HIDDEN_OPACITY, 0.4);

    // Painted through the cell's painters at the rule's opacity: a white photo over the grid comes
    // out at 40 %, its star badge and selection outline dimmed with it.
    let t = Tokens::default();
    let draw = |alpha: f32| {
        dalica_ui_kit::snapshot(200, 200, 1.0, &t, |ui| {
            let mut p = ui.painter().clone();
            p.rect_filled(ui.max_rect(), 0, t.grid_bg);
            p.multiply_opacity(alpha);
            let look = Look {
                cell: Rect::from_min_size(pos2(28.0, 28.0), vec2(144.0, 144.0)),
                image: Rect::from_min_size(pos2(28.0, 52.0), vec2(144.0, 96.0)),
                selected: true,
                anchor: true,
                hovered: false,
                badges: Badges { starred: true, ..Badges::default() },
            };
            cell::paint_under(&p, &t, &look);
            p.rect_filled(look.image, 0, egui::Color32::WHITE);
            let marks = cell::paint_over(&p, &t, &look, false);
            assert!(marks.get(Badge::Starred).is_some());
        })
        .unwrap()
    };
    let opaque = draw(cell::opacity(false, true).unwrap());
    let hidden = draw(cell::opacity(true, true).unwrap());
    let grid = t.grid_bg.r() as f32;
    let expect = |full: f32| (0.4 * full + 0.6 * grid).round();
    let image = (opaque.pixel(60, 70).unwrap()[0], hidden.pixel(60, 70).unwrap()[0]);
    assert_eq!(image.0, 255);
    // Blending rounds in the rasterizer's own way: within a few levels of 40 % over the grid.
    assert!((f32::from(image.1) - expect(255.0)).abs() <= 8.0, "the photo at 40 %: {image:?}");
    let outline = (opaque.pixel(100, 51).unwrap()[0], hidden.pixel(100, 51).unwrap()[0]);
    assert_eq!(outline.0, 255, "the anchor's white outline");
    assert!(f32::from(outline.1) < 0.5 * 255.0, "dimmed with the photo: {outline:?}");

    // Until the model has a hidden flag (M6) no photo is hidden, whatever the view says.
    let dir = tempfile::tempdir().unwrap();
    let folder = photos(dir.path(), 3, 0);
    let mut d = driver(session(dir.path(), &folder), (900, 600));
    for on in [true, false] {
        d.run("view.hiddenPictures", &json!({ "on": on })).unwrap();
        d.settle(FRAMES).unwrap();
        let all = shown(&d);
        assert_eq!(all.len(), 3);
        assert!(all.iter().all(|s| s.opacity == 1.0 && !cell::is_hidden(&photo(&d, s.photo))), "{on}");
    }
}

#[test]
fn badges_follow_the_model_star_and_held() {
    let dir = tempfile::tempdir().unwrap();
    let folder = photos(dir.path(), 4, 0);
    let mut d = driver(session(dir.path(), &folder), (900, 600));
    let ids: Vec<PhotoId> = shown(&d).iter().map(|s| s.photo).collect();
    assert!(shown(&d).iter().all(|s| s.badges == Badges::default()), "no badges at first");
    // Star the first and the third, then hold the second and the third (held in the tray). The
    // hold comes last: Ctrl+8 acts on the whole tray, held photos included.
    d.run("select.set", &json!({ "ids": [ids[0].get()] })).unwrap();
    d.key("Ctrl+8").unwrap();
    d.run("select.set", &json!({ "ids": [ids[2].get()] })).unwrap();
    d.key("Ctrl+8").unwrap();
    d.app_mut().state_mut().tray.held = vec![ids[1], ids[2]];
    d.settle(FRAMES).unwrap();
    save(&d, "cells-badges-900x600");
    let badges = |id: PhotoId| cells(&d).get(id).unwrap().badges;
    assert_eq!(badges(ids[0]), Badges { starred: true, ..Badges::default() });
    assert_eq!(badges(ids[1]), Badges { held: true, ..Badges::default() });
    assert_eq!(badges(ids[2]), Badges { held: true, starred: true, ..Badges::default() });
    assert_eq!(badges(ids[3]), Badges::default());
    // The star badge is the part agents find: inside its cell, at its bottom-right.
    let widgets = d.widgets();
    let stars: Vec<&str> = widgets.iter().map(|w| w.id.as_str()).filter(|id| id.ends_with("/star")).collect();
    assert_eq!(stars.len(), 2, "{stars:?}");
    for id in [ids[0], ids[2]] {
        let star = widgets.iter().find(|w| w.id == format!("thumb:{}/star", id.get())).unwrap();
        let cell = widgets.iter().find(|w| w.id == format!("thumb:{}", id.get())).unwrap();
        let (s, c) = (star.rect, cell.rect);
        assert!(s[0] >= c[0] && s[1] >= c[1] && s[0] + s[2] <= c[0] + c[2] && s[1] + s[3] <= c[1] + c[3], "{s:?} in {c:?}");
    }
    // The held photo with a star: two plates side by side, the star rightmost (checked in pixels:
    // the plate left of the star is dark with a light glyph).
    let shot = d.screenshot().unwrap();
    let star = widgets.iter().find(|w| w.id == format!("thumb:{}/star", ids[2].get())).unwrap().rect;
    let plate_left = (star[0] - 2.0 - 9.0, star[1] + 1.0);
    let px = shot.pixel(plate_left.0 as u32, plate_left.1 as u32).unwrap();
    assert!(px[0] < 120, "the held plate's dark corner: {px:?}");
}

#[test]
fn long_selections_and_held_lists_are_looked_up_through_a_set() {
    // More than 32 ids: the cells look them up through a set built once a pass, with the same
    // answers as the lists.
    let dir = tempfile::tempdir().unwrap();
    let folder = photos(dir.path(), 40, 0);
    let mut d = driver(session(dir.path(), &folder), (1400, 900));
    let ids: Vec<PhotoId> = d.app().session().unwrap().library().photos().map(|p| p.id).collect();
    assert_eq!(ids.len(), 40);
    let first = shown(&d)[0].photo;
    d.click(&format!("thumb:{}", first.get())).unwrap();
    d.key("Ctrl+A").unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().selection.ids.len(), 40);
    let held: Vec<PhotoId> = ids.iter().copied().filter(|i| i.get() % 7 != 0).collect();
    assert!(held.len() > 32);
    d.app_mut().state_mut().tray.held = held.clone();
    d.step();
    let all = shown(&d);
    assert!(all.len() > 10);
    for s in &all {
        assert!(s.selected, "{:?}", s.photo);
        assert_eq!(s.badges.held, held.contains(&s.photo), "{:?}", s.photo);
    }
    // Shrinking both lists below the threshold goes back to plain lookups.
    d.run("select.set", &json!({ "ids": [first.get()] })).unwrap();
    d.app_mut().state_mut().tray.held = vec![first];
    d.step();
    for s in shown(&d) {
        assert_eq!((s.selected, s.badges.held), (s.photo == first, s.photo == first), "{:?}", s.photo);
    }
}

#[test]
fn the_tooltip_shows_after_600_ms_of_virtual_time_and_not_before() {
    let dir = tempfile::tempdir().unwrap();
    let folder = photos(dir.path(), 3, 0);
    let mut d = driver(session(dir.path(), &folder), (900, 600));
    let target = thumbs(&d).remove(1);
    let id = PhotoId::new(target.trim_start_matches("thumb:").parse().unwrap());
    let rect = d.widgets().into_iter().find(|w| w.id == target).unwrap().rect;
    let in_cell = |x: u32, y: u32| (x as f32) >= rect[0] && (y as f32) >= rect[1] && (x as f32) < rect[0] + rect[2] && (y as f32) < rect[1] + rect[3];

    // The pointer arrives (frame 0) and rests: frames 1…35 are under 0.6 s, frame 36 is 0.6 s.
    d.hover(&target, 0).unwrap();
    let mut frames = 1;
    while frames < 35 {
        assert_eq!(cells(&d).tooltip(), None, "frame {frames}");
        d.step();
        frames += 1;
    }
    assert_eq!(cells(&d).tooltip(), None, "{frames} frames = {:.3} s: still under {TOOLTIP_SECONDS} s", frames as f64 / 60.0);
    let before = d.screenshot().unwrap();
    d.step();
    let (photo_id, tip) = cells(&d).tooltip().expect("the tooltip at 0.6 s");
    assert_eq!(photo_id, id);
    let p = photo(&d, id);
    assert_eq!(tip.title, p.name);
    let date = p.date.unwrap();
    for part in [MONTHS[usize::from(date.date().month()) - 1].to_string(), format!("{} × {}", p.width, p.height), "KB".to_string()] {
        assert!(tip.detail.contains(&part), "{part} in {}", tip.detail);
    }
    // It is painted (from the next frame: egui measures a new popup first): a box of new pixels
    // outside the cell.
    d.step();
    assert!(cells(&d).tooltip().is_some());
    let after = d.screenshot().unwrap();
    let mut changed = 0;
    for y in 0..after.height {
        for x in 0..after.width {
            if !in_cell(x, y) && before.pixel(x, y) != after.pixel(x, y) {
                changed += 1;
            }
        }
    }
    assert!(changed > 2000, "{changed} pixels changed outside the cell");
    save(&d, "cells-tooltip-900x600");

    // Moving away hides it; coming back waits the full 600 ms again.
    d.click(&thumbs(&d)[0]).unwrap();
    d.step();
    assert_eq!(cells(&d).tooltip(), None);
    d.hover(&target, 500).unwrap();
    assert_eq!(cells(&d).tooltip(), None, "0.5 s after coming back");
    d.wait(100).unwrap();
    assert!(cells(&d).tooltip().is_some(), "0.6 s after coming back");
}

#[test]
fn a_photo_whose_file_is_gone_shows_the_caution_tile_and_why() {
    let dir = tempfile::tempdir().unwrap();
    let folder = photos(dir.path(), 3, 0);
    let s = session(dir.path(), &folder);
    let gone = s.library().photos().find(|p| p.name == "IMG_0002.jpg").unwrap().id;
    // Gone while the library was closed: reopened (with a new thumbnail cache), its scan finds
    // no file. (A file deleted while the library is open keeps its cached thumbnails until the
    // session notices, M5.)
    drop(s);
    std::fs::remove_file(folder.join("IMG_0002.jpg")).unwrap();
    let s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    assert!(s.library().photo(gone).is_some(), "the index still lists it");
    let mut d = driver(s, (900, 600));
    let all = shown(&d);
    assert_eq!(all.len(), 3);
    for s in &all {
        assert_eq!(s.drawn == Drawn::Failed, s.photo == gone, "{:?}", s.photo);
    }
    save(&d, "cells-failed-900x600");
    d.hover(&format!("thumb:{}", gone.get()), 700).unwrap();
    let (photo_id, tip) = cells(&d).tooltip().unwrap();
    assert_eq!(photo_id, gone);
    assert_eq!(tip.title, "IMG_0002.jpg");
    assert!(tip.detail.starts_with("Cannot show this photo: ") && tip.detail.len() > 24, "{}", tip.detail);
}

#[test]
fn tooltip_text_lists_what_is_known() {
    let p: Photo = serde_json::from_value(json!({
        "id": 1, "folder": 1, "name": "IMG_0001.jpg", "size": 3_100_000, "mtime": 0,
        "width": 4000, "height": 3000, "date": "2026-06-04T19:42:00", "star": false
    }))
    .unwrap();
    let tip = cell::tooltip_text(&p, None);
    assert_eq!((tip.title.as_str(), tip.detail.as_str()), ("IMG_0001.jpg", "June 4, 2026 7:42 PM · 4000 × 3000 · 3.1 MB"));
    let unknown = Photo { width: 0, height: 0, date: None, size: 12, ..p.clone() };
    assert_eq!(cell::tooltip_text(&unknown, None).detail, "January 1, 1970 12:00 AM · 12 bytes", "the file's date when none was taken");
    assert_eq!(cell::tooltip_text(&p, Some("the file is missing")).detail, "Cannot show this photo: the file is missing");
    assert_eq!(cell::caption_text(&unknown, Caption::Resolution), Some(String::new()));
    assert_eq!(cell::caption_text(&p, Caption::Date).as_deref(), Some("June 4, 2026"));
    assert_eq!(cell::caption_text(&p, Caption::None), None);
}
