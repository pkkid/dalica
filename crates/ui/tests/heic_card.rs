//! The HEIC card (`ui-layout.md` §5.3): in a build without the HEIC decoder a section shows its
//! other photos as cells and, after them, one card two cells wide saying how many HEIC and HEIF
//! files it does not display, why and what to do; a section of only HEICs shows only the card. A
//! click on the card selects those photos; Select All and the header's count are written down in
//! `organizing.md` §5. Renders go to `target/test-output/ui/` (and `DALICA_SHOTS`).

use std::sync::Arc;

use crate::common::{FRAMES, LARGE, SMALL, new_driver, shot};
use crate::heic_hint::heic;
use dalica_engine::Session;
use dalica_image::formats::heif::{HINT, REASON};
use dalica_scenes::Scene;
use dalica_ui::App;
use dalica_ui::panels::light_table::cell::{card_id, card_title};
use dalica_ui::state::Section;
use serde_json::json;

#[test]
fn heics_are_one_two_cell_card_after_the_photos_shown() {
    assert!(!dalica_image::Format::Heif.supported(), "the UI's tests run without the `heif` feature");
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("photos");
    let (mixed, phone) = (root.join("Mixed"), root.join("Phone"));
    std::fs::create_dir_all(&mixed).unwrap();
    std::fs::create_dir_all(&phone).unwrap();
    for i in 0..3u64 {
        let jpeg = dalica_scenes::jpeg(&Scene::new(70 + i, 600, 400), 85, None).unwrap();
        std::fs::write(mixed.join(format!("IMG_000{}.jpg", i + 1)), jpeg).unwrap();
    }
    let exif = dalica_scenes::Exif { date_time_original: "2026-06-04T19:42:10".parse().ok(), ..Default::default() };
    let tiff = dalica_scenes::exif::tiff(&exif, 4032, 3024, None).unwrap();
    // `.heic` and `.heif` alike, in any case.
    for name in ["IMG_0004.HEIC", "IMG_0005.heic", "IMG_0006.heif"] {
        std::fs::write(mixed.join(name), heic(&tiff)).unwrap();
    }
    std::fs::write(phone.join("IMG_0100.HEIC"), heic(&tiff)).unwrap();
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": root })).unwrap();
    s.wait_idle();
    let folder_of = |s: &Session, path: &std::path::Path| s.library().folders().find(|f| f.path == path).map(|f| f.id).unwrap();
    let (mixed_id, phone_id) = (Section::Folder(folder_of(&s, &mixed)), Section::Folder(folder_of(&s, &phone)));
    let heics: Vec<u64> = {
        let mut v: Vec<u64> =
            s.library().photos().filter(|p| p.name.to_ascii_lowercase().contains(".hei") && p.name != "IMG_0100.HEIC").map(|p| p.id.get()).collect();
        v.sort_unstable();
        v
    };
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    // The headers date their folders by the HEICs too: the phone folder has no other photo.
    d.record_texts(true);
    d.step();
    assert!(d.drawn_texts().iter().filter(|t| t.contains("June 4, 2026")).count() >= 2, "both headers dated: {:?}", d.drawn_texts());
    d.record_texts(false);
    shot(&d, "heic-card-1400x900");

    let p = d.app().panels().unwrap();
    let lt = &p.light_table;
    let grid = lt.layout().unwrap().grid();
    // The mixed folder: its three JPEGs as cells, then one card for its three HEICs.
    let sec = p.sections.get(mixed_id).unwrap();
    assert_eq!((sec.photos.len(), sec.unshown.len()), (3, 3));
    assert!(sec.subtitle.ends_with("6 photos"), "the header counts every photo: {}", sec.subtitle);
    let shown: Vec<u64> = lt.cells.shown().iter().map(|c| c.photo.get()).collect();
    assert!(heics.iter().all(|h| !shown.contains(h)), "no cell for a HEIC: {shown:?}");
    assert_eq!(shown.len(), 3, "the three JPEGs");
    let card = lt.cells.cards().iter().find(|c| c.section == mixed_id).unwrap();
    assert_eq!(card.title, "Not displaying 3 HEIC images");
    assert_eq!(card.note, format!("{REASON}.\n{HINT}"));
    assert!(
        (card.rect.width() - (2.0 * grid.cell + grid.gap)).abs() < 0.01 && card.rect.height() == grid.cell,
        "two cells wide, one tall: {:?}",
        card.rect
    );
    let mut ids: Vec<u64> = card.photos.iter().map(|p| p.get()).collect();
    ids.sort_unstable();
    assert_eq!(ids, heics);
    // The phone folder: only the card, for its one HEIC.
    let phone_sec = p.sections.get(phone_id).unwrap();
    assert_eq!((phone_sec.photos.len(), phone_sec.unshown.len()), (0, 1));
    let card = lt.cells.cards().iter().find(|c| c.section == phone_id).unwrap();
    assert_eq!(card.title, card_title(1));
    assert_eq!(card.title, "Not displaying 1 HEIC image");

    // Select All selects the photos shown; a click on the card selects the HEICs.
    d.run("select.all", &json!({ "section": mixed_id })).unwrap();
    assert_eq!(d.state().selection.ids.len(), 3);
    d.click(&card_id(mixed_id)).unwrap();
    d.settle(FRAMES).unwrap();
    let mut selected: Vec<u64> = d.state().selection.ids.iter().map(|p| p.get()).collect();
    selected.sort_unstable();
    assert_eq!(selected, heics);
    let card = d.app().panels().unwrap().light_table.cells.cards().iter().find(|c| c.section == mixed_id).unwrap().clone();
    assert!(card.selected, "drawn selected");
    shot(&d, "heic-card-selected-1400x900");

    d.resize(SMALL.0, SMALL.1);
    d.settle(FRAMES).unwrap();
    shot(&d, "heic-card-900x600");
    let lt = &d.app().panels().unwrap().light_table;
    let grid = lt.layout().unwrap().grid();
    let card = lt.cells.cards().iter().find(|c| c.section == mixed_id).unwrap();
    let span = 2.min(grid.cols) as f32;
    assert!((card.rect.width() - (span * grid.cell + (span - 1.0) * grid.gap)).abs() < 0.01, "two cells, or the row at one column: {:?}", card.rect);
}
