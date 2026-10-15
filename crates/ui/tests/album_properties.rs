//! Album Properties (M9.17, `organizing.md` §4.2), driven headlessly by widget id on a temp
//! library of two folders:
//!
//! - opened on an album (a double-click on its header title), the draft starts from the library;
//!   OK changes the name, the date (the box off, a day typed) and the description in one undo
//!   step, which undo takes back; the tree row and the header follow;
//! - Esc (and Cancel) change nothing;
//! - OK waits for a name and, with the box off, for a date that reads as a day; turning the box on
//!   shows the newest photo's date again;
//! - the dialog matches its fingerprints at 1400 × 900 and 900 × 600
//!   (`tests/fingerprints/dialog-album-properties-<w>x<h>.json`; `DALICA_BLESS=1` stores new ones).

use std::sync::Arc;

use crate::common::{FRAMES, check_region, new_driver, put_dated_jpeg, try_widget};
use dalica_core::{AlbumId, Date, FolderId, PhotoId};
use dalica_engine::Session;
use dalica_ui::dialogs::album_properties::{BAD_DATE, DATE_ID, DESCRIPTION_ID, NAME_ID, PLACE_ID, USE_NEWEST_ID};
use dalica_ui::headless::{Click, DriverError};
use dalica_ui::state::{AlbumKey, AlbumSection, DialogKind, Section};
use dalica_ui::{App, Driver};
use serde_json::json;

const SIZES: [(u32, u32); 2] = [(1400, 900), (900, 600)];

struct Fx {
    _dir: tempfile::TempDir,
    /// "Lisbon Trip": the three photos of `Lisbon Weekend` (2026-06-04 to 06).
    album: AlbumId,
    d: Driver,
}

fn fixture((w, h): (u32, u32)) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    for (i, day) in [(2026, 6, 5), (2026, 6, 4), (2026, 6, 6)].into_iter().enumerate() {
        put_dated_jpeg(&photos.join("Lisbon Weekend").join(format!("IMG_{i:04}.jpg")), i as u64, day);
    }
    put_dated_jpeg(&photos.join("Porto").join("IMG_0000.jpg"), 10, (2026, 5, 1));
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    for f in ["Lisbon Weekend", "Porto"] {
        s.execute("folders.add", &json!({ "path": photos.join(f) })).unwrap();
    }
    s.wait_idle();
    let lisbon: FolderId = s.library().folder_by_path(&photos.join("Lisbon Weekend")).unwrap();
    let ids: Vec<u64> = s.library().photos_in(lisbon).iter().map(|p| p.id.get()).collect();
    let made = s.execute("album.create", &json!({ "name": "Lisbon Trip", "ids": ids })).unwrap();
    let album = AlbumId::parse(made["album"].as_str().unwrap()).unwrap();
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    Fx { _dir: dir, album, d }
}

fn section(album: &AlbumId) -> Section {
    Section::Album(AlbumSection::User(AlbumKey::of(album)))
}

fn info(d: &Driver, album: &AlbumId) -> dalica_library::AlbumInfo {
    d.app().session().unwrap().library().album(album).unwrap().clone()
}

fn undo_len(d: &Driver) -> usize {
    d.app().session().unwrap().undo_stack().len()
}

/// Opens Album Properties by a double-click on the album header's title.
fn open(fx: &mut Fx) {
    fx.d.click_with(&format!("section:album:{}/title", fx.album), &Click { count: 2, ..Click::default() }).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(fx.d.state().dialogs.last().map(|d| d.kind), Some(DialogKind::AlbumProperties));
    assert_eq!(fx.d.state().selection.section, Some(section(&fx.album)), "the double-click selected the album");
}

/// Replaces the text of a field: click, select all, type.
fn retype(d: &mut Driver, id: &str, text: &str) {
    d.click(id).unwrap();
    d.key("Ctrl+A").unwrap();
    d.type_text(text).unwrap();
    d.settle(FRAMES).unwrap();
}

fn ok_enabled(d: &Driver) -> bool {
    try_widget(d, "button:dialog.ok").unwrap().enabled
}

/// Properties changes the name, the date and the description in one undo step.
#[test]
fn properties_change_name_date_and_description_in_one_step() {
    let mut fx = fixture((1400, 900));
    open(&mut fx);
    let draft = fx.d.state().album_properties.clone().unwrap();
    assert_eq!(draft.album.as_ref(), Some(&fx.album));
    assert_eq!((draft.name.as_str(), draft.use_newest, draft.date.as_str()), ("Lisbon Trip", true, "2026-06-06"));
    assert!(!try_widget(&fx.d, DATE_ID).unwrap().enabled, "the date field waits while the newest photo's date is used");

    retype(&mut fx.d, NAME_ID, "Lisbon in June");
    fx.d.click(USE_NEWEST_ID).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert!(try_widget(&fx.d, DATE_ID).unwrap().enabled);
    retype(&mut fx.d, DATE_ID, "2026-06-01");
    retype(&mut fx.d, PLACE_ID, "Lisbon, Portugal");
    retype(&mut fx.d, DESCRIPTION_ID, "Trams and tiles");
    let steps = undo_len(&fx.d);
    fx.d.click("button:dialog.ok").unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert!(fx.d.state().dialogs.is_empty());
    let after = info(&fx.d, &fx.album);
    assert_eq!(after.name, "Lisbon in June");
    assert_eq!((after.date, after.use_newest), (Some(Date::new(2026, 6, 1).unwrap()), false));
    assert_eq!((after.place.as_str(), after.description.as_str()), ("Lisbon, Portugal", "Trams and tiles"));
    assert_eq!(undo_len(&fx.d), steps + 1, "one undo step");
    // The tree row and the header follow.
    assert_eq!(try_widget(&fx.d, &format!("tree:album:{}", fx.album)).unwrap().label, "Lisbon in June (3)");
    let headers = &fx.d.app().panels().unwrap().light_table.headers;
    let (date, description) = headers.shown(section(&fx.album)).unwrap();
    assert_eq!((date, description), ("Monday, June 1, 2026 · Lisbon, Portugal", "Trams and tiles"));

    fx.d.run("edit.undo", &json!({})).unwrap();
    fx.d.settle(FRAMES).unwrap();
    let back = info(&fx.d, &fx.album);
    assert_eq!((back.name.as_str(), back.date, back.description.as_str()), ("Lisbon Trip", None, ""));
}

/// Esc and Cancel close the dialog; nothing changes.
#[test]
fn esc_and_cancel_change_nothing() {
    let mut fx = fixture((1400, 900));
    let before = info(&fx.d, &fx.album);
    let steps = undo_len(&fx.d);
    for close in ["Escape", "button:dialog.cancel"] {
        open(&mut fx);
        retype(&mut fx.d, NAME_ID, "Something else");
        retype(&mut fx.d, DESCRIPTION_ID, "Not kept");
        if close == "Escape" {
            // The first Esc ends the field's edit; the next closes the dialog.
            fx.d.key("Escape").unwrap();
            fx.d.settle(FRAMES).unwrap();
            if !fx.d.state().dialogs.is_empty() {
                fx.d.key("Escape").unwrap();
            }
        } else {
            fx.d.click(close).unwrap();
        }
        fx.d.settle(FRAMES).unwrap();
        assert!(fx.d.state().dialogs.is_empty(), "{close} closes it");
        assert_eq!(fx.d.state().album_properties, None);
        assert_eq!(info(&fx.d, &fx.album), before, "{close}: nothing changed");
        assert_eq!(undo_len(&fx.d), steps, "{close}: no undo step");
    }
}

/// OK waits for a name and, with the box off, a date that reads as a day; the box on shows the
/// newest photo's date again.
#[test]
fn ok_waits_for_a_name_and_a_readable_date() {
    let mut fx = fixture((1400, 900));
    open(&mut fx);
    assert!(ok_enabled(&fx.d));
    retype(&mut fx.d, NAME_ID, "   ");
    assert!(!ok_enabled(&fx.d), "a blank name");
    assert!(matches!(fx.d.click("button:dialog.ok"), Err(DriverError::Disabled(_))));
    retype(&mut fx.d, NAME_ID, "Trip");
    fx.d.click(USE_NEWEST_ID).unwrap();
    fx.d.settle(FRAMES).unwrap();
    for bad in ["June 1", "2026-02-30"] {
        retype(&mut fx.d, DATE_ID, bad);
        assert!(!ok_enabled(&fx.d), "{bad:?}");
        assert_eq!(fx.d.state().album_properties.as_ref().unwrap().date, bad, "the field keeps what was typed");
    }
    let draft = fx.d.state().album_properties.clone().unwrap();
    assert_eq!(dalica_ui::dialogs::album_properties::ready(&draft), Err(BAD_DATE.to_string()));
    fx.d.click(USE_NEWEST_ID).unwrap();
    fx.d.settle(FRAMES).unwrap();
    let draft = fx.d.state().album_properties.clone().unwrap();
    assert_eq!((draft.use_newest, draft.date.as_str()), (true, "2026-06-06"), "the newest photo's date again");
    assert!(ok_enabled(&fx.d));
}

/// The dialog for a new album, at both sizes.
#[test]
fn the_dialog_matches_its_fingerprints_at_both_sizes() {
    for size in SIZES {
        let mut fx = fixture(size);
        let library = fx.d.app().session().unwrap().library();
        let porto: Vec<PhotoId> = library.photos().map(|p| p.id).filter(|id| !library.album_members(&fx.album).contains(id)).collect();
        assert_eq!(porto.len(), 1, "the Porto photo");
        fx.d.run("album.new", &json!({ "ids": [porto[0].get()] })).unwrap();
        fx.d.settle(FRAMES).unwrap();
        check_region(&fx.d, "dialog:albumProperties", &format!("dialog-album-properties-{}x{}", size.0, size.1));
    }
}
